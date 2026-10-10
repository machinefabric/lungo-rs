import Lungo
import Assured.Clock

/-!
A small program with claims about its exports: one proved outright, one conditional on what the
host's clock is assumed to do. The tests append to this file and change it.
-/
namespace Assured

def double (n : Nat) : Nat := n + n

/-- What `double` computes. -/
@[lungo_spec "lungo.model"]
def twice (n : Nat) : Nat := 2 * n

@[lungo_claim "lungo.equals" subject double spec twice]
theorem double_eq (n : Nat) : double n = twice n := by
  simp [double, twice]
  omega

/-- The time since `start`. -/
def elapsed (start : Nat) : Nat := now () - start

/-- Never more than the time now — on a host whose clock is monotone. -/
@[lungo_claim "lungo.law" subject elapsed]
theorem elapsed_le (_h : Monotone) (start : Nat) : elapsed start ≤ now () :=
  Nat.sub_le _ _

end Assured
