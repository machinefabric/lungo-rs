import Lungo

/-!
Deadlines on the host's clock. The clock is a facility: Lean declares its operations and the
host implements them. What the claims prove holds on any host whose clock ticks (`Ticks`) — which
the host must make true; Lean cannot check it. On a clock that never ticks, the same exports
compute something else, and the claims say nothing about them.
-/
namespace Timing

/-- The host's clock. -/
@[lungo_facility "timing.clock"]
structure Clock

/-- Ticks of the host's clock per second. -/
@[extern "timing_ticks_per_second", lungo_operation Clock]
opaque ticksPerSecond (u : Unit) : Nat

/-- The time now, in ticks of the host's clock. -/
@[extern "timing_now", lungo_operation Clock]
opaque now (u : Unit) : IO Nat

/-- What the claims assume of the host's clock: it ticks. -/
@[lungo_assumption Clock]
def Ticks : Prop := 0 < ticksPerSecond ()

def toTicks (seconds : Nat) : Nat := seconds * ticksPerSecond ()

/-- Whole seconds in `ticks`. -/
def toSeconds (ticks : Nat) : Nat := ticks / ticksPerSecond ()

@[lungo_claim "lungo.roundtrip" subject toTicks toSeconds]
theorem toSeconds_toTicks (h : Ticks) (seconds : Nat) : toSeconds (toTicks seconds) = seconds :=
  Nat.mul_div_cancel seconds h

/-- Whole seconds left before `deadline`, at time `t` (in ticks). -/
def secondsLeft (deadline t : Nat) : Nat := toSeconds (deadline - t)

/-- A deadline `seconds` after `t` has exactly `seconds` left at `t`, and never more later. -/
@[lungo_claim "lungo.law" subject secondsLeft toTicks]
theorem secondsLeft_deadline (h : Ticks) (seconds t t' : Nat) (later : t ≤ t') :
    secondsLeft (t + toTicks seconds) t = seconds ∧ secondsLeft (t + toTicks seconds) t' ≤ seconds := by
  refine ⟨?_, ?_⟩
  · simp only [secondsLeft, Nat.add_sub_cancel_left]
    exact toSeconds_toTicks h seconds
  · simp only [secondsLeft, toSeconds, toTicks]
    exact Nat.div_le_of_le_mul (by rw [Nat.mul_comm]; omega)

/-- A deadline `seconds` from now. -/
def deadlineIn (seconds : Nat) : IO Nat := do
  return (← now ()) + toTicks seconds

/-- Whole seconds left before `deadline`, now. -/
def remaining (deadline : Nat) : IO Nat := do
  return secondsLeft deadline (← now ())

end Timing
