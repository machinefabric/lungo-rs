import Lean
import Lean.Util.CollectAxioms
import LungoWorker.Cbor
import LungoWorker.BridgeIR
import LungoWorker.Diagnostics
import LungoWorker.LCNFAdapter

/-!
The assurance records of a program: what its Lean code registered with lungo's Lean library
(`@[lungo_spec]`, `@[lungo_claim]`, `@[lungo_capability]`, `@[lungo_operation]`,
`@[lungo_assumption]`, `@[lungo_role]`), read from the compiled environment and checked against
it.

The worker is not compiled against the library. It knows the library's record structures by
name and reads each record's value in the one canonical form the attributes write; any other
form is a violation. A record only says what a declaration was registered as: every fact a
claim rests on — that its evidence is a theorem, that the theorem mentions the claim's subjects
and specifications, which axioms it depends on, which assumptions it is conditional on — is
recomputed here from the environment, so a record written by hand gains nothing.

Problems are reported as data (`violations`), each with a kind the host maps to an error code;
the worker itself fails only when the environment cannot be read.
-/
namespace LungoWorker.Assurance

open Lean Meta LungoWorker.Cbor

/-- The record format this worker reads (`Lungo.Registry.schemaVersion`). -/
def supportedSchema : Nat := 1

/-- lungo's own relations, specification kinds and roles; mirrors `Lungo.Attr.Names`. -/
def claimRelations : List String :=
  ["decides", "satisfies", "refines", "preserves", "equals", "roundtrip", "law", "monitors"]
def specKinds : List String := ["relation", "contract", "model", "state", "protocol", "property"]
def roles : List String := ["implementation", "oracle", "monitor", "model"]

inductive ViolationKind where
  | malformedRecord
  | danglingReference
  | invalidClaim
  | notInStatement
  | duplicateId
  | capabilityMismatch
  | asyncInterface
  | libraryVersion
  | metadataOnlyDependency
  deriving BEq, Inhabited

def ViolationKind.toCbor : ViolationKind → Value
  | .malformedRecord => unitVariant "malformed_record"
  | .danglingReference => unitVariant "dangling_reference"
  | .invalidClaim => unitVariant "invalid_claim"
  | .notInStatement => unitVariant "not_in_statement"
  | .duplicateId => unitVariant "duplicate_id"
  | .capabilityMismatch => unitVariant "capability_mismatch"
  | .asyncInterface => unitVariant "async_interface"
  | .libraryVersion => unitVariant "library_version"
  | .metadataOnlyDependency => unitVariant "metadata_only_dependency"

structure Violation where
  kind : ViolationKind
  declaration : Option Name
  message : String

def Violation.toCbor (v : Violation) : Value :=
  obj [("kind", v.kind.toCbor), ("declaration", opt (v.declaration.map BridgeIR.name)),
    ("message", str v.message)]

/-! ## The library -/

/-- Where lungo's Lean library is in the environment. -/
structure Library where
  /-- The module defining the record structures. -/
  module : Nat
  package : String
  schemaVersion : Nat
  /-- Modules of the library that are compile-time only (`Lungo.Attr…`): never linked. -/
  barriers : Array Nat

/-- The library, when the program imports it; `packageOf` names the package owning a module. -/
def findLibrary (env : Environment) (packageOf : Nat → Option String) : Except String (Option Library) := do
  let some modIdx := env.getModuleIdxFor? `Lungo.Registry.Claim | return none
  let some package := packageOf modIdx.toNat
    | throw "lungo's Lean library (`Lungo.Registry`) is not in a package of the Lake workspace"
  let schemaVersion ← match env.find? `Lungo.Registry.schemaVersion with
    | some (.defnInfo d) =>
      match d.value with
      | .lit (.natVal n) => pure n
      | _ => throw "`Lungo.Registry.schemaVersion` is not a literal number"
    | _ => throw "lungo's Lean library defines no `Lungo.Registry.schemaVersion`"
  let mut barriers := #[]
  for h : i in [0:env.header.moduleNames.size] do
    let n := env.header.moduleNames[i]
    if (`Lungo.Attr).isPrefixOf n && packageOf i == some package then
      barriers := barriers.push i
  return some { module := modIdx.toNat, package, schemaVersion, barriers }

/-! ## Reading records -/

def recordName (decl : Name) (kind : String) : Name := .str decl s!"_lungo_{kind}"

partial def decodeName : Expr → Option Name
  | .const `Lean.Name.anonymous [] => some .anonymous
  | .app (.app (.const `Lean.Name.str []) p) (.lit (.strVal s)) => return .str (← decodeName p) s
  | .app (.app (.const `Lean.Name.num []) p) (.lit (.natVal k)) => return .num (← decodeName p) k
  | _ => none

def isNameType : Expr → Bool
  | .const `Lean.Name [] => true
  | _ => false

partial def decodeNames : Expr → Option (List Name)
  | .app (.const `List.nil [.zero]) t => if isNameType t then some [] else none
  | .app (.app (.app (.const `List.cons [.zero]) t) x) xs =>
    if isNameType t then return (← decodeName x) :: (← decodeNames xs) else none
  | _ => none

def decodeOptionName : Expr → Option (Option Name)
  | .app (.const `Option.none [.zero]) t => if isNameType t then some none else none
  | .app (.app (.const `Option.some [.zero]) t) x =>
    if isNameType t then return some (← decodeName x) else none
  | _ => none

def decodeString : Expr → Option String
  | .lit (.strVal s) => some s
  | _ => none

/-- The arguments of `mk` applied to exactly `arity` arguments. -/
def ctorArgs (mk : Name) (arity : Nat) (e : Expr) : Option (Array Expr) :=
  if e.isAppOfArity mk arity then
    match e.getAppFn with
    | .const _ [] => some e.getAppArgs
    | _ => none
  else none

structure SpecRec where
  decl : Name
  kind : String
structure ClaimRec where
  evidence : Name
  relation : String
  subjects : List Name
  specs : List Name
structure CapabilityRec where
  decl : Name
  id : String
  async : Option Name
structure OperationRec where
  decl : Name
  capability : Name
structure AssumptionRec where
  decl : Name
  capability : Name
structure RoleRec where
  decl : Name
  role : String

/-- The records of a program, by the declaration each describes. -/
structure Records where
  specs : NameMap SpecRec := {}
  claims : NameMap ClaimRec := {}
  capabilities : NameMap CapabilityRec := {}
  operations : NameMap OperationRec := {}
  assumptions : NameMap AssumptionRec := {}
  roles : NameMap RoleRec := {}
  violations : Array Violation := #[]

def Records.flag (r : Records) (kind : ViolationKind) (decl : Option Name) (message : String) : Records :=
  { r with violations := r.violations.push { kind, declaration := decl, message } }

/-- The record structure a constant's type names, if any. -/
def recordKind? (ty : Expr) : Option String :=
  match ty with
  | .const `Lungo.Registry.Spec [] => some "spec"
  | .const `Lungo.Registry.Claim [] => some "claim"
  | .const `Lungo.Registry.Capability [] => some "capability"
  | .const `Lungo.Registry.Operation [] => some "operation"
  | .const `Lungo.Registry.Assumption [] => some "assumption"
  | .const `Lungo.Registry.Role [] => some "role"
  | _ => none

/-- Reads the record constant `n` of the given kind into `r`. -/
def readRecord (r : Records) (n : Name) (kind : String) (ci : ConstantInfo) : Records := Id.run do
  let malformed (why : String) :=
    r.flag .malformedRecord (some n) s!"`{n}` is not a record lungo's Lean library writes: {why}"
  match ci with
  | .defnInfo d =>
    if !d.levelParams.isEmpty then return malformed "it has universe parameters"
    -- The record of `decl` is `decl._lungo_<kind>`.
    let target? : Option Name := match n with
      | .str p s => if s == s!"_lungo_{kind}" then some p else none
      | _ => none
    let some target := target? | return malformed s!"its name does not end in `_lungo_{kind}`"
    let named (decl : Name) (rest : Records → Records) : Records :=
      if decl != target then malformed s!"it describes `{decl}`, not `{target}`" else rest r
    return match kind with
    | "spec" =>
      match ctorArgs `Lungo.Registry.Spec.mk 2 d.value with
      | some #[a, b] =>
        match decodeName a, decodeString b with
        | some decl, some k => named decl fun r => { r with specs := r.specs.insert decl { decl, kind := k } }
        | _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Spec.mk` application"
    | "claim" =>
      match ctorArgs `Lungo.Registry.Claim.mk 4 d.value with
      | some #[a, b, c, e] =>
        match decodeName a, decodeString b, decodeNames c, decodeNames e with
        | some evidence, some relation, some subjects, some specs =>
          named evidence fun r =>
            { r with claims := r.claims.insert evidence { evidence, relation, subjects, specs } }
        | _, _, _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Claim.mk` application"
    | "capability" =>
      match ctorArgs `Lungo.Registry.Capability.mk 3 d.value with
      | some #[a, b, c] =>
        match decodeName a, decodeString b, decodeOptionName c with
        | some decl, some id, some async =>
          named decl fun r => { r with capabilities := r.capabilities.insert decl { decl, id, async } }
        | _, _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Capability.mk` application"
    | "operation" =>
      match ctorArgs `Lungo.Registry.Operation.mk 2 d.value with
      | some #[a, b] =>
        match decodeName a, decodeName b with
        | some decl, some capability =>
          named decl fun r => { r with operations := r.operations.insert decl { decl, capability } }
        | _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Operation.mk` application"
    | "assumption" =>
      match ctorArgs `Lungo.Registry.Assumption.mk 2 d.value with
      | some #[a, b] =>
        match decodeName a, decodeName b with
        | some decl, some capability =>
          named decl fun r => { r with assumptions := r.assumptions.insert decl { decl, capability } }
        | _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Assumption.mk` application"
    | "role" =>
      match ctorArgs `Lungo.Registry.Role.mk 2 d.value with
      | some #[a, b] =>
        match decodeName a, decodeString b with
        | some decl, some role => named decl fun r => { r with roles := r.roles.insert decl { decl, role } }
        | _, _ => malformed "its value is not in canonical form"
      | _ => malformed "its value is not a `Lungo.Registry.Role.mk` application"
    | _ => malformed "unknown record kind"
  | _ => return malformed "it is not a definition"

/--
Reads every record declared by a module that imports the library (directly or not). Modules are
in import order in the environment's header, so one forward pass decides which modules can hold
records.
-/
def readRecords (env : Environment) (lib : Library) : Records := Id.run do
  let n := env.header.moduleNames.size
  let mut dependsOnLibrary : Array Bool := Array.replicate n false
  let mut r : Records := {}
  for h : i in [0:n] do
    let some data := env.header.moduleData[i]? | continue
    let dep := i == lib.module || data.imports.any fun imp =>
      match env.getModuleIdx? imp.module with
      | some j => dependsOnLibrary[j.toNat]!
      | none => false
    dependsOnLibrary := dependsOnLibrary.set! i dep
    unless dep do continue
    for c in data.constNames do
      let some ci := env.find? c | continue
      if let some kind := recordKind? ci.type then
        r := readRecord r c kind ci
  return r

/-! ## Checking records against the environment -/

/-- Mirrors `Lungo.Attr.wellFormed`. -/
def wellFormed (s : String) : Bool :=
  let segmentOk (first : Bool) (seg : String) : Bool :=
    match seg.toList with
    | [] => false
    | c :: cs => c.isLower && cs.all fun d => d.isLower || d.isDigit || d == '_' || (!first && d == '-')
  match s.splitOn "." with
  | [] | [_] => false
  | first :: rest => segmentOk true first && rest.all (segmentOk false)

/-- `none` when `s` is a valid namespaced string whose `lungo` names are among `known`. -/
def kindProblem (what : String) (s : String) (known : List String) : Option String :=
  if !wellFormed s then some s!"`{s}` is not a valid {what}"
  else match s.splitOn "." with
    | "lungo" :: rest =>
      if known.contains (".".intercalate rest) then none
      else some s!"`{s}` is not a {what} lungo defines"
    | _ => none

/-- The statement after its leading `∀`s; mirrors `Lungo.Attr.conclusion`. -/
partial def conclusion : Expr → Expr
  | .forallE _ _ b _ => conclusion b
  | .mdata _ e => conclusion e
  | e => e

def mentions (e : Expr) (n : Name) : Bool := (e.find? (·.isConstOf n)).isSome

/-- Mirrors `Lungo.Attr.shapeError`. -/
def shapeProblem (relation : String) (statement : Expr) (subjects : List Name) : Option String :=
  let c := (conclusion statement).consumeMData
  let all (e : Expr) := subjects.all (mentions e)
  let none' (e : Expr) := subjects.all fun s => !mentions e s
  let arg (e : Expr) (i : Nat) := e.getArg! i
  match relation with
  | "decides" =>
    if c.isAppOfArity ``Iff 2 then
      let lhs := (arg c 0).consumeMData
      if lhs.isAppOfArity ``Eq 3 && (arg lhs 0).isConstOf ``Bool && (arg lhs 2).isConstOf ``Bool.true
          && all (arg lhs 1) then none
      else some "the left side of its `↔` is not `… = true` mentioning every subject"
    else some "it is not an `↔`"
  | "equals" => if c.isAppOfArity ``Eq 3 && all (arg c 1) then none
    else some "it is not an equation whose left side mentions every subject"
  | "roundtrip" => if c.isAppOfArity ``Eq 3 && all (arg c 1) && none' (arg c 2) then none
    else some "it is not an equation applying every subject on the left and none on the right"
  | "satisfies" =>
    if (c.isAppOfArity `Lungo.Spec.Satisfies 4 || c.isAppOfArity `Lungo.Spec.SatisfiesExcept 5)
        && all (arg c (c.getAppNumArgs - 2)) then none
    else some "it is not `Lungo.Spec.Satisfies f c` with `f` mentioning every subject"
  | "refines" => if c.isAppOfArity `Lungo.Spec.Refines 3 && all (arg c 1) then none
    else some "it is not `Lungo.Spec.Refines impl spec` with `impl` mentioning every subject"
  | "preserves" =>
    if (c.isAppOfArity `Lungo.Spec.StateSpec.Preserves 5 ||
        c.isAppOfArity `Lungo.Spec.StateSpec.Implements 5) && all (arg c 4) then none
    else some "it is not `Lungo.Spec.StateSpec.Preserves s exec` with `exec` mentioning every subject"
  | "monitors" => if c.isAppOfArity `Lungo.Monitor.Sound 4 && all (arg c 2) then none
    else some "it is not `Lungo.Monitor.Sound m p` with `m` mentioning every subject"
  | "law" => if subjects.all (mentions statement) then none else some "it does not mention every subject"
  | other => some s!"`lungo.{other}` is not a relation of lungo's"

/-- The registered assumptions among the hypotheses of `statement`: the constants in the binder
types of its leading `∀`s, as written. -/
partial def hypothesisConstants (statement : Expr) (acc : NameSet := {}) : NameSet :=
  match statement with
  | .forallE _ d b _ => hypothesisConstants b (d.getUsedConstants.foldl (·.insert ·) acc)
  | .mdata _ e => hypothesisConstants e acc
  | _ => acc

/-! ## Fingerprint material -/

/-- A canonical text of expressions: a table of their distinct subterms in post order, without
binder names or metadata, with universe parameters numbered by position. Shared subterms are
written once, so the text grows with the number of distinct subterms. -/
structure Canon where
  params : List Name
  ids : Std.HashMap Expr Nat := {}
  lines : Array String := #[]

def escape (s : String) : String :=
  s.foldl (init := "") fun acc c =>
    if c == '\\' then acc ++ "\\\\" else if c == '\n' then acc ++ "\\n" else if c == ' ' then acc ++ "\\s"
    else acc.push c

partial def canonLevel (params : List Name) : Level → Except String String
  | .zero => return "0"
  | .succ l => return s!"(+1 {← canonLevel params l})"
  | .max a b => return s!"(max {← canonLevel params a} {← canonLevel params b})"
  | .imax a b => return s!"(imax {← canonLevel params a} {← canonLevel params b})"
  | .param n => match params.idxOf? n with
    | some i => return s!"u{i}"
    | none => throw s!"universe parameter {n} is not declared"
  | .mvar _ => throw "a universe metavariable"

def binderCode : BinderInfo → String
  | .default => "d" | .implicit => "i" | .strictImplicit => "s" | .instImplicit => "c"

partial def canonExpr (e : Expr) : StateT Canon (Except String) Nat := do
  let e := e.consumeMData
  if let some i := (← get).ids[e]? then return i
  let params := (← get).params
  let line ← match e with
    | .bvar k => pure s!"b {k}"
    | .sort l => pure s!"s {← canonLevel params (l.normalize)}"
    | .const n ls => pure s!"c {escape n.toString} {" ".intercalate (← ls.mapM fun l => canonLevel params l.normalize)}"
    | .app f a => pure s!"a {← canonExpr f} {← canonExpr a}"
    | .lam _ d b bi => pure s!"l {binderCode bi} {← canonExpr d} {← canonExpr b}"
    | .forallE _ d b bi => pure s!"p {binderCode bi} {← canonExpr d} {← canonExpr b}"
    | .letE _ t v b nondep => pure s!"e {← canonExpr t} {← canonExpr v} {← canonExpr b} {nondep}"
    | .lit (.natVal k) => pure s!"n {k}"
    | .lit (.strVal s) => pure s!"t {escape s}"
    | .proj s i x => pure s!"j {escape s.toString} {i} {← canonExpr x}"
    | .fvar _ => throw "a free variable"
    | .mvar _ => throw "a metavariable"
    | .mdata .. => throw "unreachable: metadata is removed before a subterm is written"
  let s ← get
  let i := s.lines.size
  set { s with ids := s.ids.insert e i, lines := s.lines.push line }
  return i

/-- The canonical text of `exprs`, labelled. -/
def canonText (params : List Name) (header : String) (exprs : List (String × Expr)) : Except String String := do
  let go : StateT Canon (Except String) (List String) := exprs.mapM fun (label, e) => do
    return s!"{label} {← canonExpr e}"
  let (roots, s) ← go.run { params }
  return "\n".intercalate ([header, s!"levels {params.length}"] ++ s.lines.toList ++ ["roots"] ++ roots)

/-! ## Analysis -/

structure Input where
  env : Environment
  opts : Options
  lib : Library
  records : Records
  /-- The exported declarations of the program. -/
  exports : NameSet
  /-- Declarations whose compiled code is part of the program. -/
  closureNames : NameSet
  /-- The module owning each declaration's package, and its source, as the driver computes them. -/
  origin : Name → CoreM Value
  runCore : {α : Type} → CoreM α → IO (Except String α)

def sortNames (xs : Array Name) : Array Name :=
  xs.qsort fun a b => BridgeIR.nameString a < BridgeIR.nameString b

def names (xs : List Name) : Value := arr (xs.toArray.map BridgeIR.name)

/-- Why the async capability `inst`, an instance of `Lungo.Async.Interface op`, is not one a host
can serve, as `@[lungo_capability]` checks it: an operation with a field a host cannot supply, or
whose answer's type is not fixed by the operation alone. `none` when it can. -/
def asyncProblem (op inst : Name) (ctors : List Name) : MetaM (Option String) := do
  for ctor in ctors do
    let cinfo ← getConstInfoCtor ctor
    let problem ← forallTelescope cinfo.type fun xs _ => do
      for x in xs do
        let t ← inferType x
        if (← isProp t) || (← isTypeFormerType t) then
          return some s!"the field {← ppExpr x} of `{ctor}` is a proof or a type, which a host cannot supply"
      let r ← whnfD (mkApp3 (mkConst `Lungo.Async.Interface.Ret) (mkConst op) (mkConst inst) (mkAppN (mkConst ctor) xs))
      if r.isAppOf `Lungo.Async.Interface.Ret then
        return some s!"what `{ctor}` answers does not reduce to a type"
      if xs.any fun x => r.containsFVar x.fvarId! then
        return some s!"what `{ctor}` answers, {← ppExpr r}, depends on its arguments: the type of each \
          operation's answer must be fixed by the operation alone"
      return none
    if problem.isSome then return problem
  return none

/-- The C entry of extern declaration `decl`, as lungo resolves it. -/
def externSymbol? (env : Environment) (decl : Name) : Option String :=
  match getExternAttrData? env decl with
  | some data =>
    match LCNFAdapter.cEntry data.entries with
    | some (.standard _ s) => some s
    | some (.adhoc _) => some (BridgeIR.nameString decl)
    | _ => none
  | none => none

structure Output where
  value : Value
  violations : Array Violation

/-- Checks the records against the environment and encodes them. -/
def analyze (input : Input) : IO (Except String Output) := do
  let env := input.env
  let rec_ := input.records
  let mut violations := rec_.violations
  let flag (kind : ViolationKind) (decl : Name) (message : String) (vs : Array Violation) :=
    vs.push { kind, declaration := some decl, message }
  if input.lib.schemaVersion != supportedSchema then
    let message := s!"lungo's Lean library writes records in format {input.lib.schemaVersion}, and this \
      lungo reads format {supportedSchema}: use the library released with this lungo"
    violations := violations.push { kind := .libraryVersion, declaration := none, message }
  let run {α : Type} (x : CoreM α) : IO (Except String α) := input.runCore x
  let pretty (e : Expr) : CoreM String := return toString (← (ppExpr e).run')
  let isTheorem (n : Name) := env.find? n |>.any (· matches .thmInfo _)
  -- Specifications.
  let mut specsOut := #[]
  for (decl, s) in rec_.specs.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    let some ci := env.find? decl
      | violations := flag .danglingReference decl s!"the specification `{decl}` does not exist" violations; continue
    if let some why := kindProblem "specification kind" s.kind specKinds then
      violations := flag .malformedRecord decl s!"the specification `{decl}`: {why}" violations
    let exprs := [("type", ci.type)] ++ (match ci with | .defnInfo d => [("value", d.value)] | _ => [])
    let material ← match canonText ci.levelParams s!"spec {s.kind}" exprs with
      | .ok m => pure m
      | .error e => return .error s!"cannot fingerprint `{decl}`: {e}"
    let statement ← match ← run (pretty ci.type) with | .ok s => pure s | .error e => return .error e
    let origin ← match ← run (input.origin decl) with | .ok o => pure o | .error e => return .error e
    specsOut := specsOut.push (obj [("name", BridgeIR.name decl), ("kind", str s.kind),
      ("statement", str statement), ("origin", origin), ("fingerprint_material", str material)])
  -- Capabilities.
  let mut capabilitiesOut := #[]
  let mut ids : Std.HashMap String Name := {}
  for (decl, c) in rec_.capabilities.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    let some ci := env.find? decl
      | violations := flag .danglingReference decl s!"the capability `{decl}` does not exist" violations; continue
    if let some why := kindProblem "capability identifier" c.id [] then
      violations := flag .malformedRecord decl s!"the capability `{decl}`: {why}" violations
    if let some other := ids[c.id]? then
      violations := flag .duplicateId decl
        s!"the capabilities `{other}` and `{decl}` have the same identifier `{c.id}`" violations
    ids := ids.insert c.id decl
    let mut kind := unitVariant "extern"
    let mut exprs := [("type", ci.type)]
    if let some op := c.async then
      let ok := match ci.type with
        | .app (.const `Lungo.Async.Interface []) (.const o []) => o == op
        | _ => false
      unless ok do
        violations := flag .asyncInterface decl
          s!"the async capability `{decl}` is not an instance of `Lungo.Async.Interface {op}`" violations
      match env.find? op with
      | some (.inductInfo info) =>
        if info.numParams != 0 || info.numIndices != 0 then
          violations := flag .asyncInterface decl
            s!"the operations of the async capability `{decl}`, `{op}`, have parameters or indices" violations
        if ok then
          match ← run (asyncProblem op decl info.ctors).run' with
          | .ok none => pure ()
          | .ok (some why) =>
            violations := flag .asyncInterface decl s!"the async capability `{decl}`: {why}" violations
          | .error e =>
            violations := flag .asyncInterface decl s!"the async capability `{decl}` cannot be checked: {e}" violations
        let ctors := info.ctors.filterMap fun ctor => (env.find? ctor).map fun cci => (ctor, cci.type)
        exprs := exprs ++ ctors.map fun (ctor, t) => (s!"operation {escape ctor.toString}", t)
        kind := variant "async" [("op_type", BridgeIR.name op), ("operations", arr (info.ctors.toArray.map BridgeIR.name))]
      | _ =>
        violations := flag .asyncInterface decl
          s!"the operations of the async capability `{decl}`, `{op}`, are not an inductive type" violations
    let material ← match canonText ci.levelParams s!"capability {c.id}" exprs with
      | .ok m => pure m
      | .error e => return .error s!"cannot fingerprint `{decl}`: {e}"
    let origin ← match ← run (input.origin decl) with | .ok o => pure o | .error e => return .error e
    capabilitiesOut := capabilitiesOut.push (obj [("name", BridgeIR.name decl), ("id", str c.id),
      ("kind", kind), ("origin", origin), ("fingerprint_material", str material)])
  -- Operations.
  let mut operationsOut := #[]
  for (decl, o) in rec_.operations.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    let some ci := env.find? decl
      | violations := flag .danglingReference decl s!"the operation `{decl}` does not exist" violations; continue
    match rec_.capabilities.find? o.capability with
    | none =>
      violations := flag .danglingReference decl
        s!"the operation `{decl}` belongs to `{o.capability}`, which is not a capability" violations
    | some c =>
      if c.async.isSome then
        violations := flag .capabilityMismatch decl
          s!"the operation `{decl}` belongs to the async capability `{o.capability}`, whose operations are \
            the constructors of its operation type" violations
    let some symbol := externSymbol? env decl
      | violations := flag .capabilityMismatch decl
          s!"the operation `{decl}` is not an `@[extern]` declaration with an entry for C" violations; continue
    let material ← match canonText ci.levelParams s!"operation {escape symbol}" [("type", ci.type)] with
      | .ok m => pure m
      | .error e => return .error s!"cannot fingerprint `{decl}`: {e}"
    let origin ← match ← run (input.origin decl) with | .ok o => pure o | .error e => return .error e
    operationsOut := operationsOut.push (obj [("name", BridgeIR.name decl),
      ("capability", BridgeIR.name o.capability), ("symbol", str symbol),
      ("reachable", .bool (input.closureNames.contains decl)), ("origin", origin),
      ("fingerprint_material", str material)])
  -- Assumptions.
  let mut assumptionsOut := #[]
  for (decl, a) in rec_.assumptions.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    let some ci := env.find? decl
      | violations := flag .danglingReference decl s!"the assumption `{decl}` does not exist" violations; continue
    unless rec_.capabilities.contains a.capability do
      violations := flag .danglingReference decl
        s!"the assumption `{decl}` is of `{a.capability}`, which is not a capability" violations
    let isProposition ← match ← run (do
        let r : MetaM Bool := forallTelescopeReducing ci.type fun _ b => return (← whnfD b).isProp
        r.run') with
      | .ok b => pure b
      | .error e => return .error e
    unless isProposition do
      violations := flag .malformedRecord decl s!"the assumption `{decl}` is not a proposition" violations
    let exprs := [("type", ci.type)] ++ (match ci with | .defnInfo d => [("value", d.value)] | _ => [])
    let material ← match canonText ci.levelParams "assumption" exprs with
      | .ok m => pure m
      | .error e => return .error s!"cannot fingerprint `{decl}`: {e}"
    let statement ← match ← run (pretty ci.type) with | .ok s => pure s | .error e => return .error e
    let origin ← match ← run (input.origin decl) with | .ok o => pure o | .error e => return .error e
    assumptionsOut := assumptionsOut.push (obj [("name", BridgeIR.name decl),
      ("capability", BridgeIR.name a.capability), ("statement", str statement), ("origin", origin),
      ("fingerprint_material", str material)])
  -- Claims.
  let mut claimsOut := #[]
  for (evidence, c) in rec_.claims.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    let some ci := env.find? evidence
      | violations := flag .danglingReference evidence s!"the evidence `{evidence}` does not exist" violations; continue
    let .thmInfo thm := ci
      | violations := flag .invalidClaim evidence s!"the evidence of a claim must be a theorem; `{evidence}` is not" violations; continue
    let mut ok := true
    if let some why := kindProblem "claim relation" c.relation claimRelations then
      violations := flag .malformedRecord evidence s!"the claim of `{evidence}`: {why}" violations
      ok := false
    if c.subjects.isEmpty then
      violations := flag .invalidClaim evidence s!"the claim of `{evidence}` has no subject" violations
      ok := false
    for s in c.subjects do
      if !env.contains s then
        violations := flag .danglingReference evidence s!"the claim of `{evidence}` is about `{s}`, which does not exist" violations
        ok := false
      else if isTheorem s then
        violations := flag .invalidClaim evidence s!"the claim of `{evidence}` is about the theorem `{s}`; a claim is about definitions a program runs" violations
        ok := false
      else if !mentions thm.type s then
        violations := flag .notInStatement evidence s!"the statement of `{evidence}` does not mention its subject `{s}`" violations
        ok := false
    for s in c.specs do
      if !rec_.specs.contains s then
        violations := flag .danglingReference evidence s!"the claim of `{evidence}` cites `{s}`, which is not a registered specification" violations
        ok := false
      else if !mentions thm.type s then
        violations := flag .notInStatement evidence s!"the statement of `{evidence}` does not mention its specification `{s}`" violations
        ok := false
    if ok then
      if let "lungo" :: [r] := c.relation.splitOn "." then
        if let some why := shapeProblem r thm.type c.subjects then
          violations := flag .invalidClaim evidence s!"`{evidence}` does not state a `{c.relation}` claim: {why}" violations
    let hyps := hypothesisConstants thm.type
    let assumptions := sortNames (hyps.toArray.filter rec_.assumptions.contains)
    let axioms ← match ← run (collectAxioms evidence) with
      | .ok a => pure a
      | .error e => return .error s!"cannot collect the axioms of `{evidence}`: {e}"
    let material ← match canonText thm.levelParams s!"claim {c.relation}"
        ([("statement", thm.type)] ++ c.subjects.map (fun s => ("subject", mkConst s)) ++
          c.specs.map (fun s => ("spec", mkConst s))) with
      | .ok m => pure m
      | .error e => return .error s!"cannot fingerprint `{evidence}`: {e}"
    let statement ← match ← run (pretty thm.type) with | .ok s => pure s | .error e => return .error e
    let origin ← match ← run (input.origin evidence) with | .ok o => pure o | .error e => return .error e
    claimsOut := claimsOut.push (obj [
      ("evidence", BridgeIR.name evidence), ("relation", str c.relation),
      ("subjects", names c.subjects), ("specs", names c.specs),
      ("statement", str statement),
      ("assumptions", arr (assumptions.map BridgeIR.name)),
      ("evidence_trust", obj [
        ("axioms", arr (sortNames (axioms.filter (· != ``sorryAx)) |>.map BridgeIR.name)),
        ("depends_on_sorry", .bool (axioms.contains ``sorryAx))]),
      ("origin", origin), ("fingerprint_material", str material)])
  -- Roles.
  let mut rolesOut := #[]
  for (decl, r) in rec_.roles.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    unless env.contains decl do
      violations := flag .danglingReference decl s!"the role of `{decl}` names a declaration that does not exist" violations
      continue
    if let some why := kindProblem "role" r.role roles then
      violations := flag .malformedRecord decl s!"the role of `{decl}`: {why}" violations
    let origin ← match ← run (input.origin decl) with | .ok o => pure o | .error e => return .error e
    rolesOut := rolesOut.push (obj [("name", BridgeIR.name decl), ("role", str r.role),
      ("exported", .bool (input.exports.contains decl)), ("origin", origin)])
  return .ok {
    value := obj [
      ("library", obj [("package", str input.lib.package), ("schema_version", nat input.lib.schemaVersion)]),
      ("specs", arr specsOut), ("claims", arr claimsOut), ("capabilities", arr capabilitiesOut),
      ("operations", arr operationsOut), ("assumptions", arr assumptionsOut), ("roles", arr rolesOut)]
    violations
  }

end LungoWorker.Assurance
