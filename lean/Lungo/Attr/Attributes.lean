module

public meta import Lean
public import Lungo.Registry
public import Lungo.Async
public meta import Lungo.Attr.Names
public meta import Lungo.Attr.Record
public meta import Lungo.Attr.Shapes

/-!
# The `@[lungo_…]` attributes

Each attribute checks what it is given against the environment, then writes one record (see
`Lungo.Registry`). Everything an attribute refuses here, lungo's worker refuses again when it
reads the records, so a record written by hand is held to the same rules.
-/

@[expose] public section

namespace Lungo.Attr

open Lean Meta Elab

syntax (name := lungo_spec) "lungo_spec " str : attr
syntax (name := lungo_claim) "lungo_claim " str (ppSpace ident)+ : attr
syntax (name := lungo_capability) "lungo_capability " str : attr
syntax (name := lungo_operation) "lungo_operation " ident : attr
syntax (name := lungo_assumption) "lungo_assumption " ident : attr
syntax (name := lungo_role) "lungo_role " str : attr

meta def ensureGlobal (attr : String) (kind : AttributeKind) : CoreM Unit := do
  unless kind == .global do
    throwError "`@[{attr}]` cannot be `local` or `scoped`: a record is part of the module"

/-- Whether `decl` is a theorem. In a file of the module system a public theorem is exported as
an axiom, so the kind is read from the module's own (unexported) view. -/
meta def isTheorem (decl : Name) : CoreM Bool := do
  return (((← getEnv).setExporting false).findAsync? decl).any (·.kind == .thm)

/-- The capability record of `cap`, or an error naming `attr`. -/
meta def capabilityRecord (attr : String) (cap : Name) : CoreM Expr := do
  let some r ← record? cap "capability"
    | throwError "`@[{attr} {cap}]`: `{cap}` is not a capability; give it `@[lungo_capability \"ns.name\"]` first"
  return r

meta def addSpec (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_spec" kind
  let some k := stx[1].isStrLit? | throwError "`@[lungo_spec]` expects a kind, such as `\"lungo.relation\"`"
  checkKind "specification kind" k specKinds "lungo.relation"
  addRecord "lungo_spec" decl "spec" ``Lungo.Registry.Spec
    (mkApp2 (mkConst ``Lungo.Registry.Spec.mk) (nameExpr decl) (mkStrLit k))

/-- Splits `subject f g spec S T` into its subjects and specifications. -/
meta def claimParts (args : Array Syntax) : CoreM (Array Syntax × Array Syntax) := do
  let isWord (s : Syntax) (w : Name) := s.getId == w
  let some first := args[0]? | throwError "`@[lungo_claim]` expects `subject …`"
  unless isWord first `subject do
    throwError "`@[lungo_claim relation subject f … spec S …]`: the declarations it is about come \
      after `subject`"
  let mut subjects := #[]
  let mut specs := #[]
  let mut inSpecs := false
  for a in args[1:] do
    if isWord a `subject then
      throwError "`@[lungo_claim]`: `subject` is given once, before the subjects"
    else if isWord a `spec then
      if inSpecs then throwError "`@[lungo_claim]`: `spec` is given once, before the specifications"
      inSpecs := true
    else if inSpecs then specs := specs.push a
    else subjects := subjects.push a
  if subjects.isEmpty then throwError "`@[lungo_claim]` needs at least one subject after `subject`"
  if inSpecs && specs.isEmpty then throwError "`@[lungo_claim]`: no specification follows `spec`"
  return (subjects, specs)

meta def addClaim (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_claim" kind
  let some relation := stx[1].isStrLit?
    | throwError "`@[lungo_claim]` expects a relation, such as `\"lungo.decides\"`"
  checkKind "claim relation" relation claimRelations "lungo.decides"
  unless ← isTheorem decl do
    throwError "`@[lungo_claim]` is given to the theorem that proves the claim; `{decl}` is not a theorem"
  let (subjectStx, specStx) ← claimParts stx[2].getArgs
  -- The statement only: the proof may still be being checked.
  let statement := (← getConstVal decl).type
  let mut subjects := #[]
  for s in subjectStx do
    let n ← realizeGlobalConstNoOverloadWithInfo s
    if ← isTheorem n then
      throwError "`@[lungo_claim]`: the subject `{n}` is a theorem; a claim is about the definitions \
        a program runs"
    unless (statement.find? (·.isConstOf n)).isSome do
      throwError "`@[lungo_claim]`: the statement of `{decl}` does not mention its subject `{n}`"
    if subjects.contains n then throwError "`@[lungo_claim]`: the subject `{n}` is given twice"
    subjects := subjects.push n
  let mut specs := #[]
  for s in specStx do
    let n ← realizeGlobalConstNoOverloadWithInfo s
    if (← record? n "spec").isNone then
      throwError "`@[lungo_claim]`: `{n}` is not a specification; give it `@[lungo_spec \"…\"]`"
    unless (statement.find? (·.isConstOf n)).isSome do
      throwError "`@[lungo_claim]`: the statement of `{decl}` does not mention its specification `{n}`"
    if specs.contains n then throwError "`@[lungo_claim]`: the specification `{n}` is given twice"
    specs := specs.push n
  if let "lungo" :: [r] := relation.splitOn "." then
    if let some why := shapeError r statement subjects.toList then
      throwError "`{decl}` does not state a `{relation}` claim: {why}"
  addRecord "lungo_claim" decl "claim" ``Lungo.Registry.Claim
    (mkApp4 (mkConst ``Lungo.Registry.Claim.mk) (nameExpr decl) (mkStrLit relation)
      (namesExpr subjects.toList) (namesExpr specs.toList))

/-- For an instance of `Lungo.Async.Interface Op`, the operation type `Op`, after checking that
its constructors can cross to the host and that what each one answers does not depend on its
arguments. -/
meta def asyncOperations (decl : Name) : MetaM (Option Name) := do
  let ty := (← getConstVal decl).type
  let ty ← whnfR ty
  unless ty.isAppOfArity ``Lungo.Async.Interface 1 do return none
  let op := ty.appArg!
  let .const opName [] := op
    | throwError "`@[lungo_capability]`: the operations of an async capability are an inductive type \
        without parameters, not `{op}`"
  let .inductInfo info ← getConstInfo opName
    | throwError "`@[lungo_capability]`: `{opName}` is not an inductive type"
  unless info.numParams == 0 && info.numIndices == 0 do
    throwError "`@[lungo_capability]`: `{opName}` must have no parameters or indices"
  for ctor in info.ctors do
    let cinfo ← getConstInfoCtor ctor
    forallTelescope cinfo.type fun xs _ => do
      for x in xs do
        let t ← inferType x
        if (← isProp t) || (← isTypeFormerType t) then
          throwError "`@[lungo_capability]`: the field `{x}` of `{ctor}` is a proof or a type, which a \
            host cannot supply"
      let ret ← whnfD (mkApp3 (mkConst ``Lungo.Async.Interface.Ret) op (mkConst decl) (mkAppN (mkConst ctor) xs))
      if ret.isAppOf ``Lungo.Async.Interface.Ret then
        throwError "`@[lungo_capability]`: what `{ctor}` answers does not reduce to a type: {ret}"
      if xs.any fun x => ret.containsFVar x.fvarId! then
        throwError "`@[lungo_capability]`: what `{ctor}` answers depends on its arguments:\
          {indentExpr ret}\nThe type of each operation's answer must be fixed by the operation alone."
  return some opName

meta def addCapability (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_capability" kind
  let some id := stx[1].isStrLit?
    | throwError "`@[lungo_capability]` expects an identifier, such as `\"time.clock\"`"
  checkKind "capability identifier" id [] "time.clock"
  if ← isTheorem decl then
    throwError "`@[lungo_capability]` names a capability by a definition, a structure or an \
      instance of `Lungo.Async.Interface`, not by a theorem"
  let async ← (asyncOperations decl).run'
  addRecord "lungo_capability" decl "capability" ``Lungo.Registry.Capability
    (mkApp3 (mkConst ``Lungo.Registry.Capability.mk) (nameExpr decl) (mkStrLit id) (optionNameExpr async))

meta def addOperation (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_operation" kind
  let cap ← realizeGlobalConstNoOverloadWithInfo stx[1]
  let r ← capabilityRecord "lungo_operation" cap
  unless (r.getArg! 2).isAppOf ``Option.none do
    throwError "`@[lungo_operation {cap}]`: `{cap}` is an async capability, whose operations are the \
      constructors of its operation type"
  unless isExtern (← getEnv) decl do
    throwError "`@[lungo_operation]` is given to the `@[extern]` declaration the host implements; \
      `{decl}` has no `@[extern]` (write `@[extern \"symbol\"]` before `@[lungo_operation …]`)"
  addRecord "lungo_operation" decl "operation" ``Lungo.Registry.Operation
    (mkApp2 (mkConst ``Lungo.Registry.Operation.mk) (nameExpr decl) (nameExpr cap))

meta def addAssumption (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_assumption" kind
  let cap ← realizeGlobalConstNoOverloadWithInfo stx[1]
  discard <| capabilityRecord "lungo_assumption" cap
  let isProposition ← MetaM.run' do
    forallTelescopeReducing (← getConstVal decl).type fun _ b => return (← whnfD b).isProp
  unless isProposition do
    throwError "`@[lungo_assumption]` is given to a proposition (or a family of them, or a class \
      in `Prop`); `{decl}` is not one"
  addRecord "lungo_assumption" decl "assumption" ``Lungo.Registry.Assumption
    (mkApp2 (mkConst ``Lungo.Registry.Assumption.mk) (nameExpr decl) (nameExpr cap))

meta def addRole (decl : Name) (stx : Syntax) (kind : AttributeKind) : AttrM Unit := do
  ensureGlobal "lungo_role" kind
  let some role := stx[1].isStrLit? | throwError "`@[lungo_role]` expects a role, such as `\"lungo.oracle\"`"
  checkKind "role" role roles "lungo.oracle"
  if ← isTheorem decl then
    throwError "`@[lungo_role]` is given to a definition a program runs; `{decl}` is a theorem"
  addRecord "lungo_role" decl "role" ``Lungo.Registry.Role
    (mkApp2 (mkConst ``Lungo.Registry.Role.mk) (nameExpr decl) (mkStrLit role))

meta initialize
  registerBuiltinAttribute {
    name := `lungo_spec
    descr := "a specification, of the given kind"
    add := addSpec
  }
  registerBuiltinAttribute {
    name := `lungo_claim
    descr := "this theorem proves that its subjects stand in the given relation to its specifications"
    add := addClaim
  }
  registerBuiltinAttribute {
    name := `lungo_capability
    descr := "a capability the host provides"
    add := addCapability
  }
  registerBuiltinAttribute {
    name := `lungo_operation
    descr := "an operation of a capability, implemented by the host"
    add := addOperation
    applicationTime := .afterCompilation
  }
  registerBuiltinAttribute {
    name := `lungo_assumption
    descr := "a proposition assumed, never proved, of the host's implementation of a capability"
    add := addAssumption
  }
  registerBuiltinAttribute {
    name := `lungo_role
    descr := "what this definition is for: an implementation, an oracle, a monitor, a model"
    add := addRole
  }

end Lungo.Attr
