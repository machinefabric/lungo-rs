namespace Formal

inductive Op where
  | «open»
  | close
  | tick
  deriving Repr, DecidableEq

structure Sess where
  isOpen : Bool
  count : Nat
  deriving Repr, DecidableEq

def apply : Op → Sess → Option Sess
  | .«open», s =>
      if s.isOpen then
        none
      else
        some { s with isOpen := true }
  | .close, s =>
      if s.isOpen then
        some { s with isOpen := false }
      else
        none
  | .tick, s =>
      some { s with count := s.count + 1 }

def Inv (s : Sess) : Prop :=
  s.count ≥ 0

theorem apply_preserves_inv :
    Inv s →
    apply op s = some s' →
    Inv s' := by
  intro _ _
  exact Nat.zero_le _

/-- Applies a sequence of operations, stopping at the first rejected one. -/
def run : List Op → Sess → Option Sess
  | [], s => some s
  | op :: ops, s => (apply op s).bind (run ops)

end Formal
