module

/-!
# Traces and protocols

An interactive component — a protocol participant, a user interface, a device driver, a job
worker — is specified by the sequences of events it may take part in. Events are any Lean type.
A protocol allows a set of traces closed under prefixes: whatever happened so far was allowed
at the time. Allowed behaviour may be nondeterministic; the protocol constrains, it does not
choose.

A reducer is the executable side: a state and a step function consuming one event, refusing the
events it does not accept. `ReducerRefines r p` says every trace the reducer accepts is one the
protocol allows. Properties over several traces at once (two runs of a component, for
noninterference or determinism) are relations over lists of traces, `Hyperproperty`.
-/

@[expose] public section

namespace Lungo.Trace

/-- A protocol over events `ε`: the traces it allows, closed under prefixes. -/
structure Protocol (ε : Type u) where
  allowed : List ε → Prop
  nil_allowed : allowed []
  prefix_closed : ∀ t e, allowed (t ++ [e]) → allowed t

namespace Protocol

/-- The protocol allowing every trace. -/
def any (ε : Type u) : Protocol ε where
  allowed _ := True
  nil_allowed := trivial
  prefix_closed _ _ _ := trivial

/-- The protocol of traces whose every event satisfies `p`. -/
def always {ε : Type u} (p : ε → Prop) : Protocol ε where
  allowed t := ∀ e ∈ t, p e
  nil_allowed := by intro e h; cases h
  prefix_closed t e h := fun x hx => h x (List.mem_append_left _ hx)

/-- The protocol a step relation defines: every event is allowed by `next` in the state the
trace so far has reached, starting from `start`. -/
def ofSteps {ε : Type u} {σ : Type v} (start : σ) (next : σ → ε → Option σ) : Protocol ε where
  allowed t := (t.foldlM next start).isSome
  nil_allowed := rfl
  prefix_closed t e h := by
    rw [List.foldlM_append] at h
    cases hx : t.foldlM next start with
    | none => rw [hx] at h; cases h
    | some _ => rfl

/-- Traces both protocols allow. -/
def both {ε : Type u} (a b : Protocol ε) : Protocol ε where
  allowed t := a.allowed t ∧ b.allowed t
  nil_allowed := ⟨a.nil_allowed, b.nil_allowed⟩
  prefix_closed t e h := ⟨a.prefix_closed t e h.1, b.prefix_closed t e h.2⟩

/-- Every trace `a` allows, `b` allows. -/
def Within {ε : Type u} (a b : Protocol ε) : Prop :=
  ∀ t, a.allowed t → b.allowed t

theorem within_trans {ε : Type u} {a b c : Protocol ε} (hab : a.Within b) (hbc : b.Within c) :
    a.Within c :=
  fun t h => hbc t (hab t h)

end Protocol

/-- An executable component consuming events: from a state, an event is accepted (with the next
state) or refused. -/
structure Reducer (ε : Type u) (σ : Type v) where
  start : σ
  step : σ → ε → Option σ

namespace Reducer

/-- The state the reducer reaches after `t`, if it accepts every event of `t`. -/
def run {ε : Type u} {σ : Type v} (r : Reducer ε σ) (t : List ε) : Option σ :=
  t.foldlM r.step r.start

/-- The traces the reducer accepts, as a protocol. -/
def protocol {ε : Type u} {σ : Type v} (r : Reducer ε σ) : Protocol ε :=
  Protocol.ofSteps r.start r.step

theorem accepts_iff {ε : Type u} {σ : Type v} (r : Reducer ε σ) (t : List ε) :
    r.protocol.allowed t ↔ (r.run t).isSome :=
  Iff.rfl

end Reducer

/-- Every trace the reducer accepts, the protocol allows. -/
def ReducerRefines {ε : Type u} {σ : Type v} (r : Reducer ε σ) (p : Protocol ε) : Prop :=
  r.protocol.Within p

/-- A reducer refines a protocol when an invariant linking its state to the traces that reach it
holds initially, is kept by every accepted step, and implies the protocol allows the trace. -/
theorem refines_of_invariant {ε : Type u} {σ : Type v} (r : Reducer ε σ) (p : Protocol ε)
    (inv : List ε → σ → Prop)
    (start : inv [] r.start)
    (step : ∀ t s e s', inv t s → r.step s e = some s' → inv (t ++ [e]) s')
    (sound : ∀ t s, inv t s → p.allowed t) :
    ReducerRefines r p := by
  intro t h
  have key : ∀ (t : List ε) (pre : List ε) (s : σ), inv pre s →
      ∀ s', t.foldlM r.step s = some s' → inv (pre ++ t) s' := by
    intro t
    induction t with
    | nil =>
      intro pre s hs s' hrun
      simp only [List.foldlM_nil] at hrun
      cases hrun
      simpa using hs
    | cons e rest ih =>
      intro pre s hs s' hrun
      simp only [List.foldlM_cons] at hrun
      cases hstep : r.step s e with
      | none => rw [hstep] at hrun; cases hrun
      | some s₁ =>
        rw [hstep] at hrun
        have h₁ := step pre s e s₁ hs hstep
        have := ih (pre ++ [e]) s₁ h₁ s' hrun
        simpa using this
  cases hrun : t.foldlM r.step r.start with
  | none =>
    simp only [Reducer.protocol, Protocol.ofSteps, hrun] at h
    cases h
  | some s' =>
    have := key t [] r.start start s' hrun
    simpa using sound t s' (by simpa using this)

/-- A relation over several traces at once: noninterference, determinism, and other properties
no single trace can witness. -/
def Hyperproperty (ε : Type u) := List (List ε) → Prop

/-- Two runs of `r` from the same start on traces that agree on what `low` observes reach states
`observe` cannot distinguish. -/
def Noninterference {ε : Type u} {σ : Type v} {ω : Type w} (r : Reducer ε σ)
    (low : ε → Bool) (observe : σ → ω) : Prop :=
  ∀ t₁ t₂ s₁ s₂, t₁.filter low = t₂.filter low →
    r.run t₁ = some s₁ → r.run t₂ = some s₂ → observe s₁ = observe s₂

end Lungo.Trace
