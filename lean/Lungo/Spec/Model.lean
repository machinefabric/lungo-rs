module

/-!
# Behavioural models and refinement

A behavioural model says which behaviours are admitted. What a behaviour is belongs to the model:
an input/output pair, a trace of events, a pair of states, a set of outcomes. Nothing here
assumes states, steps or finiteness.

`Refines impl spec` is the default refinement: every behaviour the implementation admits, the
specification admits too. It is the relation a `lungo.refines` claim proves. A model whose
correctness is not behaviour inclusion states its own relation and registers its claims under a
relation kind of its own.
-/

@[expose] public section

namespace Lungo.Spec

/-- A set of admitted behaviours of type `β`. -/
structure BehavioralModel (β : Type u) where
  admits : β → Prop

/-- Every behaviour `impl` admits, `spec` admits. -/
def Refines {β : Type u} (impl spec : BehavioralModel β) : Prop :=
  ∀ b, impl.admits b → spec.admits b

namespace BehavioralModel

theorem refines_refl {β : Type u} (m : BehavioralModel β) : Refines m m :=
  fun _ h => h

theorem refines_trans {β : Type u} {a b c : BehavioralModel β}
    (hab : Refines a b) (hbc : Refines b c) : Refines a c :=
  fun x h => hbc x (hab x h)

/-- The behaviours of a function: the pairs of an input and the output it computes. -/
def ofFunction {α : Type u} {γ : Type v} (f : α → γ) : BehavioralModel (α × γ) where
  admits p := f p.1 = p.2

/-- The behaviours a relation between inputs and outputs allows. -/
def ofRelation {α : Type u} {γ : Type v} (r : α → γ → Prop) : BehavioralModel (α × γ) where
  admits p := r p.1 p.2

/-- A function refines a relation exactly when every output it computes is allowed. -/
theorem function_refines_relation {α : Type u} {γ : Type v} (f : α → γ) (r : α → γ → Prop) :
    Refines (ofFunction f) (ofRelation r) ↔ ∀ a, r a (f a) := by
  constructor
  · intro h a
    exact h (a, f a) rfl
  · intro h p hp
    cases p with
    | mk a c =>
      simp only [ofFunction] at hp
      subst hp
      exact h a

/-- Behaviours both models admit. -/
def inter {β : Type u} (a b : BehavioralModel β) : BehavioralModel β where
  admits x := a.admits x ∧ b.admits x

theorem refines_inter {β : Type u} {impl a b : BehavioralModel β}
    (ha : Refines impl a) (hb : Refines impl b) : Refines impl (a.inter b) :=
  fun x h => ⟨ha x h, hb x h⟩

end BehavioralModel

end Lungo.Spec
