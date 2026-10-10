import Lungo

/-!
Acme's semantic model of cost, a Lake package of its own. It defines what Acme means by a cost
bound, a specification kind (`acme.cost-model`) and a claim relation (`acme.cost-bound`) — lungo
needs no change to carry them: any namespace but `lungo` is a package's own.
-/
namespace Acme

/-- `steps` never exceeds `bound`. -/
def CostBound {α : Type} (steps : α → Nat) (bound : α → Nat) : Prop := ∀ a, steps a ≤ bound a

/-- At most `k` steps per unit of size, and `k` more: linear in `size`. -/
@[lungo_spec "acme.cost-model"]
def Linear {α : Type} (size : α → Nat) (k : Nat) (steps : α → Nat) : Prop :=
  CostBound steps fun a => k * size a + k

/-- Whether a run of `steps` stayed within `budget`: what Acme's services check at run time. -/
def withinBudget (steps budget : Nat) : Bool := steps ≤ budget

end Acme
