import Lungo

namespace Assured

/-- The host's clock. -/
@[lungo_capability "assured.clock"]
structure Clock

/-- The time now, from the host. -/
@[extern "assured_now", lungo_operation Clock]
opaque now (u : Unit) : Nat

/-- What claims assume of the host's clock: later readings are no smaller. -/
@[lungo_assumption Clock]
def Monotone : Prop := ∀ u v : Unit, now u ≤ now v

end Assured
