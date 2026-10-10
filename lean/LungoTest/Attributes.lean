import Lungo

/-!
Every refusal of the `@[lungo_…]` attributes, and the records they write when they accept.
-/

namespace LungoTest

open Lungo.Spec

/-! ## Specifications -/

@[lungo_spec "lungo.relation"] def Even (n : Nat) : Prop := n % 2 = 0
@[lungo_spec "acme.cost-bound"] def Cheap (n : Nat) : Prop := n < 10
def Odd (n : Nat) : Prop := n % 2 = 1

/--
error: `Lungo.relation` is not a valid specification kind: it must be two or more `.`-separated segments of lowercase letters, digits and `_` (and `-` after the first), each starting with a letter, such as `lungo.relation`
-/
#guard_msgs in
@[lungo_spec "Lungo.relation"] def BadKind : Prop := True

/--
error: `lungo.theorem` is not a specification kind lungo defines; lungo's are `lungo.relation`, `lungo.contract`, `lungo.model`, `lungo.state`, `lungo.protocol`, `lungo.property`. A specification kind of your own belongs to a namespace of your own
-/
#guard_msgs in
@[lungo_spec "lungo.theorem"] def UnknownKind : Prop := True

/--
error: `relation` is not a valid specification kind: it must be two or more `.`-separated segments of lowercase letters, digits and `_` (and `-` after the first), each starting with a letter, such as `lungo.relation`
-/
#guard_msgs in
@[lungo_spec "relation"] def NoNamespace : Prop := True

/-- error: `LungoTest.Even` already has `@[lungo_spec]`: a declaration is registered once -/
#guard_msgs in
attribute [lungo_spec "lungo.relation"] Even

/-- error: `@[lungo_spec]` cannot be `local` or `scoped`: a record is part of the module -/
#guard_msgs in
attribute [local lungo_spec "lungo.relation"] Odd

/--
error: `@[lungo_spec]` must be given where `Nat.add` is declared (module `Init.Prelude`): lungo reads a declaration's records from the module that declares it
-/
#guard_msgs in
attribute [lungo_spec "lungo.relation"] Nat.add

/-! ## Claims -/

def isEven (n : Nat) : Bool := n % 2 == 0

@[lungo_claim "lungo.decides" subject isEven spec Even]
theorem isEven_decides (n : Nat) : isEven n = true ↔ Even n := by simp [isEven, Even]

/--
error: `@[lungo_claim]` is given to the theorem that proves the claim; `LungoTest.notATheorem` is not a theorem
-/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven spec Even]
def notATheorem : Bool := isEven 2

theorem isEven_two : isEven 2 = true := rfl

/--
error: `@[lungo_claim]`: the subject `LungoTest.isEven_two` is a theorem; a claim is about the definitions a program runs
-/
#guard_msgs in
@[lungo_claim "lungo.law" subject isEven_two]
theorem aboutATheorem : isEven 2 = true := isEven_two

/--
error: `@[lungo_claim]`: the statement of `LungoTest.doesNotMention` does not mention its subject `LungoTest.isEven`
-/
#guard_msgs in
@[lungo_claim "lungo.law" subject isEven]
theorem doesNotMention : 2 + 2 = 4 := rfl

/--
error: `@[lungo_claim]`: `LungoTest.Odd` is not a specification; give it `@[lungo_spec "…"]`
-/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven spec Odd]
theorem unregisteredSpec (n : Nat) : isEven n = true ↔ ¬ Odd n := by simp [isEven, Odd]

/--
error: `@[lungo_claim]`: the statement of `LungoTest.specNotMentioned` does not mention its specification `LungoTest.Cheap`
-/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven spec Cheap]
theorem specNotMentioned (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

/--
error: `@[lungo_claim relation subject f … spec S …]`: the declarations it is about come after `subject`
-/
#guard_msgs in
@[lungo_claim "lungo.decides" isEven]
theorem noSubjectWord (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

/-- error: `@[lungo_claim]`: no specification follows `spec` -/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven spec]
theorem emptySpecs (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

/-- error: `@[lungo_claim]`: the subject `LungoTest.isEven` is given twice -/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven isEven]
theorem twice (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

/--
error: `LungoTest.isEven_decides` already has `@[lungo_claim]`: a declaration is registered once
-/
#guard_msgs in
attribute [lungo_claim "lungo.decides" subject isEven spec Even] isEven_decides

/--
error: `lungo.guarantees` is not a claim relation lungo defines; lungo's are `lungo.decides`, `lungo.satisfies`, `lungo.refines`, `lungo.preserves`, `lungo.equals`, `lungo.roundtrip`, `lungo.law`, `lungo.monitors`. A claim relation of your own belongs to a namespace of your own
-/
#guard_msgs in
@[lungo_claim "lungo.guarantees" subject isEven]
theorem unknownRelation (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

-- Shapes of lungo's relations.

/--
error: `LungoTest.decidesNotIff` does not state a `lungo.decides` claim: it must be an `↔` whose left side is `… = true`
-/
#guard_msgs in
@[lungo_claim "lungo.decides" subject isEven spec Even]
theorem decidesNotIff (n : Nat) (h : isEven n = true) : Even n := (isEven_decides n).1 h

/--
error: `LungoTest.equalsNotEq` does not state a `lungo.equals` claim: it must be an equation whose left side mentions every subject
-/
#guard_msgs in
@[lungo_claim "lungo.equals" subject isEven spec Even]
theorem equalsNotEq (n : Nat) : isEven n = true ↔ Even n := isEven_decides n

def double (n : Nat) : Nat := 2 * n
def half (n : Nat) : Nat := n / 2

@[lungo_claim "lungo.roundtrip" subject double half]
theorem half_double (n : Nat) : half (double n) = n := by simp [half, double]

/--
error: `LungoTest.roundtripMentionsSubject` does not state a `lungo.roundtrip` claim: it must be an equation whose left side applies every subject and whose right side mentions none of them, such as `decode (encode x) = some x`
-/
#guard_msgs in
@[lungo_claim "lungo.roundtrip" subject double half]
theorem roundtripMentionsSubject (n : Nat) : half (double n) = half (double n) := rfl

@[lungo_spec "lungo.contract"] def doubleContract : Contract Nat Nat where
  pre _ := True
  post n m := m = n + n

@[lungo_claim "lungo.satisfies" subject double spec doubleContract]
theorem double_satisfies : Satisfies double doubleContract := by
  intro n _; simp [doubleContract, double]; omega

/--
error: `@[lungo_claim]`: the statement of `LungoTest.satisfiesOtherFunction` does not mention its subject `LungoTest.half`
-/
#guard_msgs in
@[lungo_claim "lungo.satisfies" subject half spec doubleContract]
theorem satisfiesOtherFunction : Satisfies double doubleContract := double_satisfies

/--
error: `LungoTest.refinesNotRefines` does not state a `lungo.refines` claim: it must be `Lungo.Spec.Refines impl spec` with `impl` mentioning every subject
-/
#guard_msgs in
@[lungo_claim "lungo.refines" subject double]
theorem refinesNotRefines (n : Nat) : double n = 2 * n := rfl

/--
error: `LungoTest.preservesNotPreserves` does not state a `lungo.preserves` claim: it must be `Lungo.Spec.StateSpec.Preserves s exec` or `Lungo.Spec.StateSpec.Implements s exec` with `exec` mentioning every subject
-/
#guard_msgs in
@[lungo_claim "lungo.preserves" subject double]
theorem preservesNotPreserves (n : Nat) : double n = 2 * n := rfl

/--
error: `LungoTest.monitorsNotSound` does not state a `lungo.monitors` claim: it must be `Lungo.Monitor.Sound m p` with `m` mentioning every subject
-/
#guard_msgs in
@[lungo_claim "lungo.monitors" subject double]
theorem monitorsNotSound (n : Nat) : double n = 2 * n := rfl

/-! ## Facilities, operations and assumptions -/

@[lungo_facility "time.clock"] def Clock : Unit := ()

@[extern "lungotest_now", lungo_operation Clock] opaque now : Unit → Nat

@[lungo_assumption Clock] def Monotone : Prop := ∀ u, now u ≤ now u

class LawfulClock : Prop where
  steady : ∀ u, now u = now u

attribute [lungo_assumption Clock] LawfulClock

/--
error: `lungo.clock`: the `lungo` namespace is lungo's own, and holds no facility identifier; use a namespace of your own, such as `time.clock`
-/
#guard_msgs in
@[lungo_facility "lungo.clock"] def ReservedId : Unit := ()

/--
error: `Clock` is not a valid facility identifier: it must be two or more `.`-separated segments of lowercase letters, digits and `_` (and `-` after the first), each starting with a letter, such as `time.clock`
-/
#guard_msgs in
@[lungo_facility "Clock"] def BadId : Unit := ()

/--
error: `@[lungo_facility]` names a facility by a definition, a structure or an instance of `Lungo.Async.Interface`, not by a theorem
-/
#guard_msgs in
@[lungo_facility "time.clock"] theorem facilityTheorem : True := trivial

/--
error: `@[lungo_operation]` is given to the `@[extern]` declaration the host implements; `LungoTest.notExtern` has no `@[extern]` (write `@[extern "symbol"]` before `@[lungo_operation …]`)
-/
#guard_msgs in
@[lungo_operation Clock] def notExtern (u : Unit) : Nat := 0

/--
error: `@[lungo_operation LungoTest.Even]`: `LungoTest.Even` is not a facility; give it `@[lungo_facility "ns.name"]` first
-/
#guard_msgs in
@[extern "lungotest_other", lungo_operation Even] opaque notAFacility : Unit → Nat

/--
error: `@[lungo_assumption]` is given to a proposition (or a family of them, or a class in `Prop`); `LungoTest.notAProposition` is not one
-/
#guard_msgs in
@[lungo_assumption Clock] def notAProposition : Nat := 0

/--
error: `@[lungo_assumption LungoTest.Even]`: `LungoTest.Even` is not a facility; give it `@[lungo_facility "ns.name"]` first
-/
#guard_msgs in
@[lungo_assumption Even] def assumesNotAFacility : Prop := True

inductive StoreOp where
  | get (key : String)
  | put (key : String) (value : Nat)

@[lungo_facility "store.kv"] instance storeInterface : Lungo.Async.Interface StoreOp where
  Ret | .get _ => Option Nat | .put _ _ => Unit

/--
error: `@[lungo_operation LungoTest.storeInterface]`: `LungoTest.storeInterface` is an async facility, whose operations are the constructors of its operation type
-/
#guard_msgs in
@[extern "lungotest_async", lungo_operation storeInterface] opaque asyncOperation : Unit → Nat

inductive DependentOp where
  | read (n : Nat)

/--
error: `@[lungo_facility]`: what `LungoTest.DependentOp.read` answers depends on its arguments:
  Fin (n + 1)
The type of each operation's answer must be fixed by the operation alone.
-/
#guard_msgs in
@[lungo_facility "store.dependent"] instance dependentInterface : Lungo.Async.Interface DependentOp where
  Ret | .read n => Fin (n + 1)

inductive ProofOp where
  | check (n : Nat) (h : n > 0)

/--
error: `@[lungo_facility]`: the field `h` of `LungoTest.ProofOp.check` is a proof or a type, which a host cannot supply
-/
#guard_msgs in
@[lungo_facility "store.proof"] instance proofInterface : Lungo.Async.Interface ProofOp where
  Ret _ := Bool

inductive ParamOp (α : Type) where
  | take (a : α)

/--
error: `@[lungo_facility]`: the operations of an async facility are an inductive type without parameters, not `ParamOp Nat`
-/
#guard_msgs in
@[lungo_facility "store.param"] instance paramInterface : Lungo.Async.Interface (ParamOp Nat) where
  Ret _ := Unit

/-! ## Roles -/

@[lungo_role "lungo.oracle"] def reference (n : Nat) : Nat := double n

/--
error: `lungo.helper` is not a role lungo defines; lungo's are `lungo.implementation`, `lungo.oracle`, `lungo.monitor`, `lungo.model`. A role of your own belongs to a namespace of your own
-/
#guard_msgs in
@[lungo_role "lungo.helper"] def unknownRole (n : Nat) : Nat := n

/--
error: `@[lungo_role]` is given to a definition a program runs; `LungoTest.roleTheorem` is a theorem
-/
#guard_msgs in
@[lungo_role "lungo.oracle"] theorem roleTheorem : True := trivial

/-! ## The records -/

/--
info: def LungoTest.isEven_decides._lungo_claim : Lungo.Registry.Claim :=
{ evidence := `LungoTest.isEven_decides, relation := "lungo.decides", subjects := [`LungoTest.isEven],
  specs := [`LungoTest.Even] }
-/
#guard_msgs in
#print isEven_decides._lungo_claim

/--
info: def LungoTest.storeInterface._lungo_facility : Lungo.Registry.Facility :=
{ decl := `LungoTest.storeInterface, id := "store.kv", async := some `LungoTest.StoreOp }
-/
#guard_msgs in
#print storeInterface._lungo_facility

end LungoTest
