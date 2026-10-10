import Lungo
import Polyglot.Functions
import Polyglot.Host

/-!
What is proved of the program's exports, as lungo carries it into every package: claims with and
without an assumption about the host.
-/
namespace Polyglot

open Lungo.Spec

@[lungo_claim "lungo.law" subject factorial]
theorem factorial_pos : ∀ n, 0 < factorial n
  | 0 => Nat.one_pos
  | n + 1 => Nat.mul_pos (Nat.succ_pos n) (factorial_pos n)

@[lungo_claim "lungo.roundtrip" subject Tree.mirror]
theorem Tree.mirror_mirror {α : Type} : ∀ t : Tree α, t.mirror.mirror = t
  | .leaf => rfl
  | .node l v r => by simp [Tree.mirror, Tree.mirror_mirror l, Tree.mirror_mirror r]

/-- Natural division: what `divide` computes when it can. -/
@[lungo_spec "lungo.model"]
def natDiv (a b : Nat) : Nat := a / b

@[lungo_claim "lungo.equals" subject divide spec natDiv]
theorem divide_ok (a b : Nat) (h : b ≠ 0) : divide a b = .ok (natDiv a b) := by
  simp [divide, natDiv, h]

/-- The scaled sum of one number grows with the number — on a host whose scaler is monotone. -/
@[lungo_claim "lungo.law" subject scaledSum]
theorem scaledSum_singleton_mono (h : ScalesMonotonically) (a b : Nat) (hab : a ≤ b) :
    scaledSum [a] ≤ scaledSum [b] := by
  simpa [scaledSum] using h a b hab

end Polyglot
