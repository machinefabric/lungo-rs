module

public import Lungo

/-!
The attributes in a file of the module system: records are written exposed, so lungo reads them
from importers whatever the scope of the declaration they describe.
-/

public section

namespace LungoTest.Module

@[lungo_spec "lungo.relation"] def Small (n : Nat) : Prop := n < 4

@[expose] def isSmall (n : Nat) : Bool := n < 4

@[lungo_claim "lungo.decides" subject isSmall spec Small]
theorem isSmall_decides (n : Nat) : isSmall n = true ↔ Small n := by simp [isSmall, Small]

@[lungo_capability "test.log"] def Log : Unit := ()

@[extern "lungotest_log", lungo_operation Log] opaque log : String → Unit

end LungoTest.Module
