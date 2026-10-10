module

/-!
# The records lungo reads

Every `@[lungo_…]` attribute validates its declaration and then adds one internal,
`noncomputable` constant beside it — `decl._lungo_spec`, `evidence._lungo_claim`, … — whose
value is one of the structures below, written in a single canonical form. lungo's worker reads
these constants from the compiled environment of the project; it is not compiled against this
library and never runs it.

A record only says what a declaration was registered as. Whether a claim holds is decided by
the environment the record is read from: the evidence must be a theorem the kernel checked, its
statement must mention the claim's subjects and specifications, and the axioms it depends on
are computed, never declared. A record written by hand instead of by its attribute is read the
same way and checked the same way.

The structures have no executable code and no instances: they are a format, not an API.
-/

@[expose] public section

namespace Lungo.Registry

/-- The version of the record format. lungo's worker refuses a library whose version it does not
implement. -/
def schemaVersion : Nat := nat_lit 1

/-- `@[lungo_spec kind]`: `decl` is a specification of the given kind. -/
structure Spec where
  decl : Lean.Name
  kind : String

/-- `@[lungo_claim relation subject … spec …]` on the theorem `evidence`: it proves that the
`subjects` (executable definitions) stand in `relation` to the `specs`. -/
structure Claim where
  evidence : Lean.Name
  relation : String
  subjects : List Lean.Name
  specs : List Lean.Name

/-- `@[lungo_facility id]`: `decl` names a facility the host provides. `async` is the
operation type when `decl` is an instance of `Lungo.Async.Interface`; the facility's
operations are then that type's constructors. -/
structure Facility where
  decl : Lean.Name
  id : String
  async : Option Lean.Name

/-- `@[lungo_operation facility]`: the `@[extern]` declaration `decl` is one of the operations
of `facility`, implemented by the host. -/
structure Operation where
  decl : Lean.Name
  facility : Lean.Name

/-- `@[lungo_assumption facility]`: the proposition `decl` is assumed of the host's
implementation of `facility`, never proved. -/
structure Assumption where
  decl : Lean.Name
  facility : Lean.Name

/-- `@[lungo_role role]`: `decl` plays `role` (an implementation, an oracle, a monitor, a model). -/
structure Role where
  decl : Lean.Name
  role : String

end Lungo.Registry
