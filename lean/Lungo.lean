module

public import Lungo.Core
public import Lungo.Attr

/-!
# lungo's Lean library

Import `Lungo` to state, in Lean, what lungo should carry into the packages it generates:

- `@[lungo_spec kind]` marks a specification;
- `@[lungo_claim relation subject … spec …]` on a theorem claims what it proves of executable
  definitions;
- `@[lungo_facility id]`, `@[lungo_operation C]` and `@[lungo_assumption C]` group the externs the
  host implements into facilities and state what proofs assume of them;
- `@[lungo_role role]` says what an exported definition is for (an oracle, a monitor, …).

`Lungo.Spec`, `Lungo.Trace`, `Lungo.Monitor` and `Lungo.Async` hold the definitions those claims
are usually stated with. See <https://machinefabric.com/lungo/docs/reference/lean-library>.
-/
