module

/-!
# Contracts

A contract states what a function may assume of its input and what it guarantees of its output.
`Satisfies f c` is the proposition a `lungo.satisfies` claim proves; `SatisfiesExcept` is the
same for a function that reports failure through `Except`, where the contract also says which
inputs it may refuse.
-/

@[expose] public section

namespace Lungo.Spec

/-- What a function of `α` to `β` assumes of its input (`pre`) and guarantees of its output
(`post`, which may relate output to input). -/
structure Contract (α : Type u) (β : Type v) where
  pre : α → Prop
  post : α → β → Prop

/-- `f` meets contract `c`: on every input satisfying the precondition, its output satisfies the
postcondition. -/
def Satisfies {α : Type u} {β : Type v} (f : α → β) (c : Contract α β) : Prop :=
  ∀ a, c.pre a → c.post a (f a)

/-- A contract for a function that may refuse its input: `refuses` says which inputs it may
refuse, and with what error. -/
structure ExceptContract (α : Type u) (ε : Type w) (β : Type v) extends Contract α β where
  refuses : α → ε → Prop

/-- `f` meets contract `c`: on every input satisfying the precondition, it either returns an
output satisfying the postcondition or refuses the input as the contract allows. -/
def SatisfiesExcept {α : Type u} {ε : Type w} {β : Type v} (f : α → Except ε β)
    (c : ExceptContract α ε β) : Prop :=
  ∀ a, c.pre a →
    match f a with
    | .ok b => c.post a b
    | .error e => c.refuses a e

namespace Contract

/-- A stronger contract: it assumes no more and guarantees no less. -/
def Strengthens {α : Type u} {β : Type v} (strong weak : Contract α β) : Prop :=
  (∀ a, weak.pre a → strong.pre a) ∧ (∀ a b, weak.pre a → strong.post a b → weak.post a b)

theorem satisfies_weaken {α : Type u} {β : Type v} {f : α → β} {strong weak : Contract α β}
    (h : Satisfies f strong) (s : Strengthens strong weak) : Satisfies f weak :=
  fun a ha => s.2 a (f a) ha (h a (s.1 a ha))

/-- The contract of two functions in sequence: `f` then `g`. -/
def seq {α : Type u} {β : Type v} {γ : Type w} (cf : Contract α β) (cg : Contract β γ) :
    Contract α γ where
  pre a := cf.pre a
  post a c := ∃ b, cf.post a b ∧ cg.post b c

theorem satisfies_comp {α : Type u} {β : Type v} {γ : Type w} {f : α → β} {g : β → γ}
    {cf : Contract α β} {cg : Contract β γ}
    (hf : Satisfies f cf) (hg : Satisfies g cg) (bridge : ∀ a b, cf.post a b → cg.pre b) :
    Satisfies (g ∘ f) (cf.seq cg) :=
  fun a ha => ⟨f a, hf a ha, hg (f a) (bridge a (f a) (hf a ha))⟩

end Contract

end Lungo.Spec
