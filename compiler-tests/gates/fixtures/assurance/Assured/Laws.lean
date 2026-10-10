import Assured

/-! Claims kept out of the program: lungo reads them when this is an assurance module. -/
namespace Assured

@[lungo_claim "lungo.law" subject double]
theorem double_even (n : Nat) : double n % 2 = 0 := by
  simp [double]
  omega

end Assured
