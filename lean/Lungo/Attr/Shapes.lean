module

public meta import Lean
import Lungo.Spec.Contract
import Lungo.Spec.Model
import Lungo.Spec.State
import Lungo.Monitor

/-!
# The shapes of lungo's relations

A claim under one of lungo's relations must be stated in that relation's shape, so that the
relation name means the same thing in every package. The checks are syntactic: they look at the
statement as written, after its leading `∀`s, by the constants at its head. lungo's worker runs
the same checks on what it reads, so a record written without its attribute cannot claim a
relation its statement does not have.

| relation | the statement, after its `∀`s |
| --- | --- |
| `lungo.decides` | `f … = true ↔ P …`, the left side mentioning every subject |
| `lungo.equals` | `lhs = rhs`, `lhs` mentioning every subject |
| `lungo.roundtrip` | `lhs = rhs`, `lhs` mentioning every subject and `rhs` none |
| `lungo.satisfies` | `Lungo.Spec.Satisfies f c` or `Lungo.Spec.SatisfiesExcept f c`, `f` mentioning every subject |
| `lungo.refines` | `Lungo.Spec.Refines impl spec`, `impl` mentioning every subject |
| `lungo.preserves` | `Lungo.Spec.StateSpec.Preserves s exec` or `….Implements s exec`, `exec` mentioning every subject |
| `lungo.monitors` | `Lungo.Monitor.Sound m p`, `m` mentioning every subject |
| `lungo.law` | anything mentioning every subject |
-/

@[expose] public section

namespace Lungo.Attr

open Lean

/-- The statement after its leading `∀`s, with their variables left loose. -/
meta def conclusion : Expr → Expr
  | .forallE _ _ b _ => conclusion b
  | .mdata _ e => conclusion e
  | e => e

meta def mentionsAll (e : Expr) (subjects : List Name) : Bool :=
  subjects.all fun s => (e.find? (·.isConstOf s)).isSome

meta def mentionsNone (e : Expr) (subjects : List Name) : Bool :=
  subjects.all fun s => (e.find? (·.isConstOf s)).isNone

/-- `none` when `statement` has the shape relation `lungo.<relation>` requires of a claim about
`subjects`, otherwise why it has not. -/
meta def shapeError (relation : String) (statement : Expr) (subjects : List Name) : Option String :=
  let c := (conclusion statement).consumeMData
  let arg (e : Expr) (i : Nat) := e.getArg! i
  match relation with
  | "decides" =>
    if c.isAppOfArity ``Iff 2 then
      let lhs := (arg c 0).consumeMData
      if lhs.isAppOfArity ``Eq 3 && (arg lhs 0).isConstOf ``Bool && (arg lhs 2).isConstOf ``Bool.true
          && mentionsAll (arg lhs 1) subjects then none
      else some "the left side of its `↔` must be `… = true`, mentioning every subject"
    else some "it must be an `↔` whose left side is `… = true`"
  | "equals" =>
    if c.isAppOfArity ``Eq 3 && mentionsAll (arg c 1) subjects then none
    else some "it must be an equation whose left side mentions every subject"
  | "roundtrip" =>
    if c.isAppOfArity ``Eq 3 && mentionsAll (arg c 1) subjects && mentionsNone (arg c 2) subjects
    then none
    else some "it must be an equation whose left side applies every subject and whose right side \
      mentions none of them, such as `decode (encode x) = some x`"
  | "satisfies" =>
    if (c.isAppOfArity ``Lungo.Spec.Satisfies 4 || c.isAppOfArity ``Lungo.Spec.SatisfiesExcept 5)
        && mentionsAll (arg c (c.getAppNumArgs - 2)) subjects then none
    else some "it must be `Lungo.Spec.Satisfies f c` or `Lungo.Spec.SatisfiesExcept f c` with `f` \
      mentioning every subject"
  | "refines" =>
    if c.isAppOfArity ``Lungo.Spec.Refines 3 && mentionsAll (arg c 1) subjects then none
    else some "it must be `Lungo.Spec.Refines impl spec` with `impl` mentioning every subject"
  | "preserves" =>
    if (c.isAppOfArity ``Lungo.Spec.StateSpec.Preserves 5 ||
        c.isAppOfArity ``Lungo.Spec.StateSpec.Implements 5) && mentionsAll (arg c 4) subjects then none
    else some "it must be `Lungo.Spec.StateSpec.Preserves s exec` or \
      `Lungo.Spec.StateSpec.Implements s exec` with `exec` mentioning every subject"
  | "monitors" =>
    if c.isAppOfArity ``Lungo.Monitor.Sound 4 && mentionsAll (arg c 2) subjects then none
    else some "it must be `Lungo.Monitor.Sound m p` with `m` mentioning every subject"
  | "law" =>
    if mentionsAll statement subjects then none else some "it must mention every subject"
  | other => some s!"`lungo.{other}` is not a relation of lungo's"

end Lungo.Attr
