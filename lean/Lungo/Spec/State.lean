module

/-!
# Abstract state

A stateful component specified by what may hold initially, what always holds, and which steps it
may take: from a state, on an input, producing an output, to a next state. The state is any
Lean type — a map, a graph, a history — and is never enumerated.

`Preserves` is the proposition a `lungo.preserves` claim proves of an executable step function:
from any state satisfying the invariant, every step it takes satisfies the invariant again.
`Implements` additionally says the executable step is one the specification allows.
-/

@[expose] public section

namespace Lungo.Spec

/-- A stateful component: its states `σ`, inputs `ι` and outputs `ο`. -/
structure StateSpec (σ : Type u) (ι : Type v) (ο : Type w) where
  init : σ → Prop
  invariant : σ → Prop
  step : σ → ι → ο → σ → Prop

namespace StateSpec

/-- The executable `exec` keeps the invariant: from a state satisfying it, the state after any
input satisfies it again, and every initial state satisfies it. -/
def Preserves {σ : Type u} {ι : Type v} {ο : Type w} (s : StateSpec σ ι ο)
    (exec : σ → ι → ο × σ) : Prop :=
  (∀ x, s.init x → s.invariant x) ∧
    (∀ x i, s.invariant x → s.invariant (exec x i).2)

/-- The executable `exec` takes only steps the specification allows, from states satisfying the
invariant. -/
def Implements {σ : Type u} {ι : Type v} {ο : Type w} (s : StateSpec σ ι ο)
    (exec : σ → ι → ο × σ) : Prop :=
  ∀ x i, s.invariant x → s.step x i (exec x i).1 (exec x i).2

/-- The state after feeding `inputs` to `exec` from `x`, with the outputs it produced. -/
def run {σ : Type u} {ι : Type v} {ο : Type w} (exec : σ → ι → ο × σ) :
    σ → List ι → List ο × σ
  | x, [] => ([], x)
  | x, i :: is =>
    let (o, y) := exec x i
    let (os, z) := run exec y is
    (o :: os, z)

/-- A step function that keeps the invariant keeps it over any run. -/
theorem preserves_run {σ : Type u} {ι : Type v} {ο : Type w} {s : StateSpec σ ι ο}
    {exec : σ → ι → ο × σ} (h : s.Preserves exec) :
    ∀ (is : List ι) x, s.invariant x → s.invariant (run exec x is).2
  | [], _, hx => hx
  | i :: is, x, hx => by
    simp only [run]
    exact preserves_run h is (exec x i).2 (h.2 x i hx)

/-- Every state reached from an initial state satisfies the invariant. -/
theorem reachable_invariant {σ : Type u} {ι : Type v} {ο : Type w} {s : StateSpec σ ι ο}
    {exec : σ → ι → ο × σ} (h : s.Preserves exec) (is : List ι) {x : σ} (hx : s.init x) :
    s.invariant (run exec x is).2 :=
  preserves_run h is x (h.1 x hx)

end StateSpec

end Lungo.Spec
