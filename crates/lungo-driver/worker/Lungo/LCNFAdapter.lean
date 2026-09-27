import Lean
import Lungo.BridgeIR
import Lungo.Diagnostics

/-!
Version-specific adapter from Lean 4.34.1's compiler output to Bridge IR.

Lean's LCNF pipeline ends in impure LCNF, which `Lean.IR.ToIR` lowers one-to-one into `Lean.IR`
declarations. Lean persists that final form for every module (`.ir` for `module` files, the
`.olean` for others); it is what Lean's interpreter executes and what separate compilation links
against. The adapter reads it for the executable closure of the requested roots — including
`Init`, `Std`, and package code — so every executable dependency is emitted regardless of
whether it is public.
-/
namespace Lungo.LCNFAdapter

open Lean Lean.IR

structure IRIndex where
  env : Environment
  cache : Std.HashMap Nat (Std.HashMap Name Decl) := {}

/-- The final compiler declarations of module `modIdx`, keyed by name. -/
def moduleDecls (idx : IRIndex) (modIdx : Nat) : Std.HashMap Name Decl × IRIndex :=
  match idx.cache[modIdx]? with
  | some m => (m, idx)
  | none =>
    let entries := declMapExt.getModuleIREntries idx.env modIdx
    let m := entries.foldl (init := {}) fun m d => m.insert d.name d
    (m, { idx with cache := idx.cache.insert modIdx m })

def moduleName (env : Environment) (modIdx : Nat) : Name :=
  env.header.moduleNames[modIdx]!

/-- Looks up the executable compiler declaration of `n`. -/
def find (idx : IRIndex) (n : Name) : WorkerM (Decl × Nat × IRIndex) := do
  let some modIdx := idx.env.getModuleIdxFor? n
    | fail .adapter s!"compiler declaration '{n}' is referenced but not owned by any imported module" (some n.toString)
  let (m, idx) := moduleDecls idx modIdx.toNat
  match m[n]? with
  | some (.extern _ _ _ { entries := [.opaque] }) =>
    fail .adapter s!"module {moduleName idx.env modIdx.toNat} provides only an opaque signature for '{n}'; \
      its executable IR (.ir) is missing — rebuild the module with Lake" (some n.toString)
  | some d => return (d, modIdx.toNat, idx)
  | none =>
    fail .adapter s!"'{n}' has no executable compiler output in module {moduleName idx.env modIdx.toNat}" (some n.toString)

mutual
partial def blockRefs (b : FnBody) (acc : Array Name) : Array Name :=
  match b with
  | .vdecl _ _ e k =>
    let acc := match e with
      | .fap c _ | .pap c _ => acc.push c
      | _ => acc
    blockRefs k acc
  | .jdecl _ _ v k => blockRefs k (blockRefs v acc)
  | .case _ _ _ alts => alts.foldl (fun acc a => blockRefs a.body acc) acc
  | .set _ _ _ k | .setTag _ _ k | .uset _ _ _ k | .sset _ _ _ _ _ k
  | .inc _ _ _ _ k | .dec _ _ _ _ k | .del _ k => blockRefs k acc
  | .ret _ | .jmp .. | .unreachable => acc
end

def declRefs : Decl → Array Name
  | .fdecl _ _ _ b _ => blockRefs b #[]
  | .extern .. => #[]

/--
The source-level constant a compiler declaration was derived from: compiler-generated
auxiliaries (`_boxed`, `_redArg`, `_closed_n`, lambda-lifted and specialized code, `_unsafe_rec`)
are attributed to the closest enclosing constant.
-/
partial def origin (env : Environment) (n : Name) : Option Name :=
  if env.contains n then some n
  else match n with
    | .anonymous => none
    | .str p _ | .num p _ => origin env p

/-- An initialization action of a module, in declaration order. -/
inductive Initializer where
  /-- `initialize do ...`: an `IO Unit` action run for its effects. -/
  | io (decl : Name)
  /-- `initialize x : T ← act`: `decl` holds the value produced by running `initFn`. -/
  | value (decl : Name) (initFn : Name)

def Initializer.decls : Initializer → Array Name
  | .io d => #[d]
  | .value d f => #[d, f]

/--
The initializers module `modIdx` runs when the program starts in initialization phase `phases`,
mirroring the module initialization emitted by Lean's C backend: builtin and regular `[init]`
declarations in declaration order; the runtime phase excludes `meta` declarations.
-/
def initializers (env : Environment) (modIdx : Nat) (phases : IRPhases) : Array Initializer := Id.run do
  let some data := env.header.moduleData[modIdx]? | return #[]
  let mut out := #[]
  for n in data.constNames do
    -- The runtime phase leaves out `meta` (compile-time) declarations.
    if phases == .runtime && isMarkedMeta env n then continue
    -- Programs initialize with `builtin = true`, so builtin initializers run as well, and a
    -- builtin initialization function takes precedence (as in Lean's `emitDeclInit`).
    if isIOUnitBuiltinInitFn env n || isIOUnitInitFn env n then
      out := out.push (.io n)
    else if let some f := getBuiltinInitFnNameFor? env n <|> getInitFnNameFor? env n then
      out := out.push (.value n f)
  return out

/--
Maps each external symbol provided by a Lean definition (`@[export sym] def f ...`) to that
definition. Lean's runtime implements some `@[extern]` symbols this way; for them the
implementation is compiled Lean code rather than a runtime primitive.
-/
def exportedSymbols (env : Environment) : Std.HashMap String Name := Id.run do
  let mut m := {}
  for i in [0:env.header.moduleNames.size] do
    for (decl, sym) in exportAttr.ext.getModuleEntries env i do
      m := m.insert (sym.toString (escape := false)) decl
  return m

/-- The C-backend extern entry Lean's own emitter would select for `entries`. -/
def cEntry (entries : List ExternEntry) : Option ExternEntry :=
  getExternEntryForAux `c entries

/-- The Lean definition implementing extern declaration `d`, if its symbol is `@[export]`ed. -/
def exportedBy (exports : Std.HashMap String Name) : Decl → Option Name
  | .extern f _ _ data =>
    match cEntry data.entries with
    | some (.standard _ sym) => (exports[sym]?).filter (· != f)
    | _ => none
  | _ => none

structure Closure where
  /-- Reachable compiler declarations with their owning module index. -/
  decls : Array (Decl × Nat)
  index : IRIndex

/-- All compiler declarations reachable from `roots`, including the Lean definitions that
implement `@[export]`ed extern symbols. -/
def closure (idx : IRIndex) (exports : Std.HashMap String Name) (roots : Array Name) : WorkerM Closure := do
  let mut idx := idx
  let mut seen : NameSet := {}
  let mut work := roots.reverse
  let mut out := #[]
  while h : work.size > 0 do
    let n := work.back
    work := work.pop
    if seen.contains n then continue
    seen := seen.insert n
    let (d, modIdx, idx') ← find idx n
    idx := idx'
    out := out.push (d, modIdx)
    for r in (declRefs d).reverse do
      unless seen.contains r do work := work.push r
    if let some impl := exportedBy exports d then
      unless seen.contains impl do work := work.push impl
  return { decls := out.qsort (fun a b => BridgeIR.nameString a.1.name < BridgeIR.nameString b.1.name), index := idx }

open Cbor in
def encodeDecl (env : Environment) (exports : Std.HashMap String Name) (d : Decl) (modIdx : Nat) :
    Except String Value := do
  let owner := BridgeIR.name (moduleName env modIdx)
  let originName := opt ((origin env d.name).map BridgeIR.name)
  match d with
  | .fdecl f xs ty b _ =>
    return obj [
      ("name", BridgeIR.name f), ("module", owner), ("origin", originName),
      ("params", arr (← xs.mapM BridgeIR.param)), ("result", ← BridgeIR.irType ty),
      ("body", variant "function" [("block", ← BridgeIR.block b)])
    ]
  | .extern f xs ty data =>
    let selected ← match cEntry data.entries with
      | some e => pure (BridgeIR.externEntry e)
      | none => throw s!"extern '{f}' has no entry for the C backend: {data.entries.length} entries"
    return obj [
      ("name", BridgeIR.name f), ("module", owner), ("origin", originName),
      ("params", arr (← xs.mapM BridgeIR.param)), ("result", ← BridgeIR.irType ty),
      ("body", variant "extern" [("entries", arr (data.entries.toArray.map BridgeIR.externEntry)),
        ("selected", selected), ("exported_by", opt ((exportedBy exports d).map BridgeIR.name))])
    ]

end Lungo.LCNFAdapter
