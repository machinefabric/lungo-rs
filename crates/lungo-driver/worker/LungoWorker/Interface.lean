import Lean
import Lean.Compiler.LCNF.ToImpureType
import Lean.Util.CollectAxioms
import LungoWorker.Cbor
import LungoWorker.BridgeIR
import LungoWorker.Diagnostics

/-!
The public interface of the generated Rust: which declarations receive facades, the Rust-facing
shape of their Lean types, the runtime layout of the inductive types those facades expose, and
the trust metadata of each export.

Facade types are computed from the elaborated Lean types together with Lean's own compiler
layout information (`ctorLayoutExt`, `impureTypeExt`, trivial-structure info), so that facade
conversions agree with the representation the compiled code uses. Types that cannot be soundly
represented as plain Rust data — dependent families, types carrying proofs, higher-kinded or
otherwise non-first-order types — are exposed as opaque Lean values rather than rejected.
-/
namespace LungoWorker.Interface

open Lean Meta LungoWorker.Cbor

inductive FType where
  | builtin (name : String)
  | option (t : FType)
  | list (t : FType)
  | array (t : FType)
  | prod (a b : FType)
  | except (e a : FType)
  | io (a : FType)
  | eio (e a : FType)
  | baseIO (a : FType)
  | function (params : Array FType) (result : FType)
  | param (idx : Nat)
  | inductive (name : Name) (args : Array FType)
  | opaque (head : Option Name) (leanType : String)
  /-- `Lungo.Async.Program op α`: a computation asking the host to perform operations of type
  `op`, whose constructor `i` is answered with a value of `rets[i]`, ending with an `α`. -/
  | async (op : Name) (rets : Array FType) (result : FType)
  deriving Inhabited

partial def FType.toCbor : FType → Value
  | .builtin n => unitVariant n
  | .option t => .map #[("option", t.toCbor)]
  | .list t => .map #[("list", t.toCbor)]
  | .array t => .map #[("array", t.toCbor)]
  | .prod a b => .map #[("prod", arr #[a.toCbor, b.toCbor])]
  | .except e a => variant "except" [("error", e.toCbor), ("value", a.toCbor)]
  | .io a => .map #[("io", a.toCbor)]
  | .eio e a => variant "eio" [("error", e.toCbor), ("value", a.toCbor)]
  | .baseIO a => .map #[("base_io", a.toCbor)]
  | .function ps r => variant "function" [("params", arr (ps.map FType.toCbor)), ("result", r.toCbor)]
  | .param i => .map #[("param", nat i)]
  | .inductive n args => variant "inductive" [("name", BridgeIR.name n), ("args", arr (args.map FType.toCbor))]
  | .opaque head t => variant "opaque" [("head", opt (head.map BridgeIR.name)), ("lean_type", str t)]
  | .async op rets r => variant "async" [("op", BridgeIR.name op), ("rets", arr (rets.map FType.toCbor)),
      ("result", r.toCbor)]

structure State where
  /-- Inductive types referenced by facades that must be described, in discovery order. -/
  pending : Array Name := #[]
  seen : NameSet := {}
  /-- The described types exposed as opaque values: nominal, but not first-order data. -/
  opaqueTypes : NameSet := {}

abbrev FacadeM := StateRefT State MetaM

def builtinName? : Name → Option String
  | ``Nat => some "nat"
  | ``Int => some "int"
  | ``Bool => some "bool"
  | ``UInt8 => some "uint8"
  | ``UInt16 => some "uint16"
  | ``UInt32 => some "uint32"
  | ``UInt64 => some "uint64"
  | ``USize => some "usize"
  | ``Int8 => some "int8"
  | ``Int16 => some "int16"
  | ``Int32 => some "int32"
  | ``Int64 => some "int64"
  | ``ISize => some "isize"
  | ``Float => some "float"
  | ``Float32 => some "float32"
  | ``Char => some "char"
  | ``String => some "string"
  | ``Unit => some "unit"
  | ``PUnit => some "unit"
  | ``ByteArray => some "byte_array"
  | ``FloatArray => some "float_array"
  | _ => none

def pretty (e : Expr) : MetaM String := do
  return toString (← ppExpr e)

/-- Whether `t` (a binder or field type) has no runtime representation. -/
def isErasedType (t : Expr) : MetaM Bool := do
  return (← isProp t) || (← isTypeFormerType t)

/-- Whether inductive `info` has no indices and only type parameters. -/
def hasOnlyTypeParams (info : InductiveVal) : MetaM Bool := do
  if info.numIndices != 0 then return false
  forallBoundedTelescope info.type info.numParams fun xs _ => do
    for x in xs do
      let t ← whnfD (← inferType x)
      unless t.isSort && !t.isProp do return false
    return true

/--
Whether inductive `n` can be exposed as generated Rust data: no indices, only type
parameters, and constructors whose fields all have runtime representations (a field erased by
the compiler is a proof or a type, which Rust code could not supply soundly).
-/
def isFirstOrderInductive (n : Name) : MetaM Bool := do
  let .inductInfo info ← getConstInfo n | return false
  if info.isUnsafe then return false
  unless ← hasOnlyTypeParams info do return false
  for ctor in info.ctors do
    let layout ← Compiler.LCNF.getCtorLayout ctor
    for field in layout.fieldInfo do
      match field with
      | .erased | .void => return false
      | _ => pure ()
  return true

partial def facade (tparams : Array FVarId) (ty : Expr) (fuel : Nat := 64) : FacadeM FType := do
  let ty := (← instantiateMVars ty).headBeta.consumeMData
  if let .fvar id := ty then
    if let some i := tparams.findIdx? (· == id) then return .param i
  let fn := ty.getAppFn
  let args := ty.getAppArgs
  let recur (t : Expr) : FacadeM FType := facade tparams t fuel
  match fn with
  | .const n _ =>
    if let some b := builtinName? n then
      if args.isEmpty || n == ``PUnit then return .builtin b
    match n, args with
    | ``Decidable, #[_] => return .builtin "bool"
    | ``Option, #[a] => return .option (← recur a)
    | ``List, #[a] => return .list (← recur a)
    | ``Array, #[a] => return .array (← recur a)
    | ``Prod, #[a, b] => return .prod (← recur a) (← recur b)
    | ``Except, #[e, a] => return .except (← recur e) (← recur a)
    | ``IO, #[a] => return .io (← recur a)
    | ``EIO, #[e, a] => return .eio (← recur e) (← recur a)
    | ``BaseIO, #[a] => return .baseIO (← recur a)
    | `Lungo.Async.Program, #[op, inst, a] =>
      -- An async program: the host answers each operation with the type the capability's
      -- `Interface` instance gives that constructor.
      let .const opName [] := op.consumeMData
        | throwError "an async program's operations must be an inductive type without parameters, not {op}"
      let some (.inductInfo info) := (← getEnv).find? opName
        | throwError "the operations of an async program, {opName}, are not an inductive type"
      unless info.numParams == 0 && info.numIndices == 0 && (← isFirstOrderInductive opName) do
        throwError "the operations of an async program, {opName}, must be an inductive type without \
          parameters or indices whose fields a host can supply"
      let mut rets := #[]
      for ctor in info.ctors do
        let cinfo ← getConstInfoCtor ctor
        let ret ← forallTelescope cinfo.type fun xs _ => do
          let r ← whnfD (mkApp3 (mkConst `Lungo.Async.Interface.Ret) op inst (mkAppN (mkConst ctor) xs))
          if xs.any fun x => r.containsFVar x.fvarId! then
            throwError "what the operation {ctor} is answered with, {r}, depends on its arguments"
          if r.isAppOf `Lungo.Async.Interface.Ret then
            throwError "what the operation {ctor} is answered with does not reduce to a type: {r}"
          recur r
        rets := rets.push ret
      let s ← get
      unless s.seen.contains opName do
        set { s with seen := s.seen.insert opName, pending := s.pending.push opName }
      return .async opName rets (← recur a)
    | _, _ =>
      let env ← getEnv
      if let some (.inductInfo info) := env.find? n then
        if args.size == info.numParams && (← isFirstOrderInductive n) then
          let fargs ← args.mapM recur
          let s ← get
          unless s.seen.contains n do
            set { s with seen := s.seen.insert n, pending := s.pending.push n }
          return .inductive n fargs
        -- Exposed as an opaque value, but still a nominal type: it is described (with its
        -- layout, marked opaque) so that bindings give it a type of its own and packages that
        -- share it can check they agree on its layout.
        if args.size == info.numParams && (← hasOnlyTypeParams info) then
          let s ← get
          unless s.seen.contains n do
            set { s with seen := s.seen.insert n, pending := s.pending.push n, opaqueTypes := s.opaqueTypes.insert n }
        return .opaque n (← pretty ty)
      if fuel > 0 then
        if let some ty' ← unfoldDefinition? ty then
          return ← facade tparams ty' (fuel - 1)
      return .opaque n (← pretty ty)
  | .forallE .. =>
    forallTelescope ty fun xs result => do
      -- Effectful function values take the world token as a further argument; they are exposed
      -- as opaque values rather than as callable closures.
      let head := result.getAppFn
      if head.isConstOf ``IO || head.isConstOf ``EIO || head.isConstOf ``BaseIO then
        return .opaque none (← pretty ty)
      let mut ps := #[]
      for x in xs do
        let t ← inferType x
        if ← isErasedType t then continue
        if result.containsFVar x.fvarId! then
          return .opaque none (← pretty ty)
        ps := ps.push (← recur t)
      return .function ps (← recur result)
  | _ => return .opaque none (← pretty ty)

/-- How a constructor field is stored, from Lean's compiler layout. -/
def fieldKind : Compiler.LCNF.CtorFieldInfo → Except String Value
  | Compiler.LCNF.CtorFieldInfo.object i _ => return .map #[("object", nat i)]
  | Compiler.LCNF.CtorFieldInfo.usize i => return .map #[("usize", nat i)]
  | Compiler.LCNF.CtorFieldInfo.scalar sz off ty => do
    return variant "scalar" [("size", nat sz), ("offset", nat off), ("ty", ← BridgeIR.irType (IR.toIRType ty))]
  | Compiler.LCNF.CtorFieldInfo.erased => return unitVariant "erased"
  | Compiler.LCNF.CtorFieldInfo.void => return unitVariant "void"

/-- Describes a first-order inductive type referenced by a facade. -/
def describeInductive (n : Name) : FacadeM Value := do
  let .inductInfo info ← getConstInfo n | throwError "'{n}' is not an inductive type"
  let repr ← Compiler.LCNF.nameToImpureType n
  let repr ← ofExcept (BridgeIR.irType (IR.toIRType repr))
  let trivial ← Compiler.LCNF.hasTrivialImpureStructure? n
  let isStruct := isStructure (← getEnv) n
  forallBoundedTelescope info.type info.numParams fun params _ => do
    let tparams := params.map (·.fvarId!)
    let mut paramNames := #[]
    for p in params do
      paramNames := paramNames.push (str (← p.fvarId!.getUserName).toString)
    let mut ctors := #[]
    for ctor in info.ctors do
      let cinfo ← getConstInfoCtor ctor
      let layout ← Compiler.LCNF.getCtorLayout ctor
      let ctorType ← instantiateForall cinfo.type params
      let fields ← forallTelescope ctorType fun xs _ => do
        let mut fields := #[]
        for h : i in [0:xs.size] do
          let x := xs[i]
          let some kind := layout.fieldInfo[i]? | throwError "layout of '{ctor}' has no field {i}"
          -- A field without a runtime representation (a proof or a type, in an opaque type) is
          -- recorded by its statement; exploring it would describe propositions as data.
          let fty ← match kind with
            | .erased | .void => pure (FType.opaque none (← pretty (← inferType x)))
            | _ => facade tparams (← inferType x)
          fields := fields.push (obj [
            ("name", str (← x.fvarId!.getUserName).eraseMacroScopes.toString),
            ("ty", fty.toCbor),
            ("kind", ← ofExcept (fieldKind kind))
          ])
        return fields
      ctors := ctors.push (obj [
        ("name", BridgeIR.name ctor),
        ("tag", nat layout.ctorInfo.cidx),
        ("size", nat layout.ctorInfo.size),
        ("usize", nat layout.ctorInfo.usize),
        ("ssize", nat layout.ctorInfo.ssize),
        ("fields", arr fields)
      ])
    return obj [
      ("name", BridgeIR.name n),
      ("opaque", .bool ((← get).opaqueTypes.contains n)),
      ("params", arr paramNames),
      ("repr", repr),
      ("trivial", opt (trivial.map fun t => obj [("ctor", BridgeIR.name t.ctorName), ("field", nat t.fieldIdx)])),
      ("structure", .bool isStruct),
      ("ctors", arr ctors)
    ]

/-- Describes every inductive type queued by facade computations, including those reached
while describing other types. -/
def describePending : FacadeM (Array Value) := do
  let mut out := #[]
  let mut i := 0
  while i < (← get).pending.size do
    let n := (← get).pending[i]!
    out := out.push (← describeInductive n)
    i := i + 1
  return out

/-- One parameter of a facade function, aligned with the compiler declaration's parameters. -/
inductive FParam where
  /-- A runtime argument supplied by the Rust caller. -/
  | value (name : String) (ty : FType)
  /-- A parameter erased by the compiler (type, proof) or the `IO` world token. -/
  | erased

structure Signature where
  typeParams : Array String
  params : Array FParam
  result : FType
  /-- Number of Lean binders the compiler declaration takes beyond the facade parameters. -/
  leanType : String

/--
Computes the facade signature of `decl`, whose compiler declaration has parameters `irParams`.
The compiler declaration's arity may exceed the syntactic arity of the Lean type (for example
the `IO` world token, or monads that unfold to functions); binders are then taken from the
unfolded type.
-/
partial def signature (decl : Name) (irParams : Array IR.Param) : FacadeM Signature := do
  let info ← getConstInfo decl
  let leanType ← pretty info.type
  let rec go (ty : Expr) (i : Nat) (tparams : Array FVarId) (tnames : Array String)
      (acc : Array FParam) : FacadeM Signature := do
    if h : i < irParams.size then
      let p := irParams[i]
      let ty := ty.consumeMData
      match ty with
      | .forallE n d b bi =>
        withLocalDecl n bi d fun x => do
          let isTypeParam := (← whnfD d).isSort && !(← whnfD d).isProp
          let tparams' := if isTypeParam then tparams.push x.fvarId! else tparams
          let tnames' := if isTypeParam then tnames.push n.eraseMacroScopes.toString else tnames
          let fp ← match p.ty with
            | .erased | .void => pure FParam.erased
            | _ => pure (FParam.value n.eraseMacroScopes.toString (← facade tparams d))
          go (b.instantiate1 x) (i + 1) tparams' tnames' (acc.push fp)
      | _ =>
        let fn := ty.getAppFn
        let isIO := fn.isConstOf ``IO || fn.isConstOf ``EIO || fn.isConstOf ``BaseIO
        if isIO && i + 1 == irParams.size && p.ty matches .void | .erased then
          return { typeParams := tnames, params := acc.push .erased, result := ← facade tparams ty, leanType }
        match ← unfoldDefinition? ty with
        | some ty' => go ty' i tparams tnames acc
        | none =>
          let ty' ← whnfD ty
          if ty'.isForall then go ty' i tparams tnames acc
          else throwError "the compiled arity of '{decl}' ({irParams.size}) exceeds the arity of its type {leanType}"
    else
      return { typeParams := tnames, params := acc, result := ← facade tparams ty, leanType }
  go info.type 0 #[] #[] #[]

def FParam.toCbor : FParam → Value
  | .value n t => variant "value" [("name", str n), ("ty", t.toCbor)]
  | .erased => unitVariant "erased"

/-- Compiler-generated constructions that never receive automatic facades. -/
def isGeneratedConstruction (env : Environment) (n : Name) : Bool :=
  match n with
  | .str p s =>
    (env.isConstructor p && s == "elim") ||
    ((env.find? p matches some (.inductInfo _)) &&
      ["ctorElim", "ctorElimType", "ctorIdx", "toCtorIdx", "noConfusion", "noConfusionType",
       "casesOn", "recOn", "rec", "below", "brecOn", "binductionOn", "ibelow", "sizeOf_spec"].contains s)
  | _ => false

/-- Whether `n` should receive a facade when its module is exported wholesale. -/
def isAutoExportCandidate (env : Environment) (n : Name) : MetaM Bool := do
  if n.isInternalDetail || isPrivateName n then return false
  -- Auxiliary definitions nested inside an instance (its methods) belong to the instance.
  let rec underInstance : Name → Bool
    | .str p _ | .num p _ => isInstanceCore env p || underInstance p
    | .anonymous => false
  if underInstance n then return false
  if isGeneratedConstruction env n then return false
  if isAuxRecursor env n || isNoConfusion env n then return false
  if env.isProjectionFn n || env.isConstructor n then return false
  if isInstanceCore env n then return false
  if ← Meta.isMatcher n then return false
  -- Syntax extensions (`syntax`, `notation`, `macro`, ...) define their parsers as syntax node
  -- kinds, and `meta` definitions exist for compile time: neither is part of the program's API.
  if Parser.isValidSyntaxNodeKind env n || isMarkedMeta env n then return false
  match env.find? n with
  | some (.defnInfo _) | some (.opaqueInfo _) => return true
  | _ => return false

/--
Checks that Lean represents `Lungo.Async.Program` as the runtime reads it: `done` is constructor 0
with its value as object field 0, `call` is constructor 1 with the operation and the
continuation as object fields 0 and 1 (the parameters, the instance among them, are not stored).
-/
def checkProgramLayout : MetaM Unit := do
  let done ← Compiler.LCNF.getCtorLayout `Lungo.Async.Program.done
  let call ← Compiler.LCNF.getCtorLayout `Lungo.Async.Program.call
  let doneOk := done.ctorInfo.cidx == 0 && match done.fieldInfo with
    | #[.object 0 _] => true
    | _ => false
  let callOk := call.ctorInfo.cidx == 1 && match call.fieldInfo with
    | #[.object 0 _, .object 1 _] => true
    | _ => false
  unless doneOk && callOk do
    throwError "Lean represents `Lungo.Async.Program` in a layout this adapter does not read"

end LungoWorker.Interface
