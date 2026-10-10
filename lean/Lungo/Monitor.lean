module

public import Lungo.Trace

/-!
# Runtime monitors

A monitor is an executable checker a host feeds events into, one at a time, keeping the
monitor's state between them. Each event is accepted (giving the next state) or reported as a
violation with a reason; what to do about a violation is the host's decision.

`Sound m p` says the monitor accepts exactly the traces protocol `p` allows, so a violation it
reports is a real violation of `p` and an accepted trace is one `p` allows. It is the
proposition a `lungo.monitors` claim proves. The combinators below build sound monitors from an
event predicate, from a reducer, and from two monitors at once.
-/

@[expose] public section

namespace Lungo

open Lungo.Trace

/-- A monitor over events `ε` with state `σ`. -/
structure Monitor (ε : Type u) (σ : Type v) where
  start : σ
  step : σ → ε → Except String σ

namespace Monitor

/-- The state after feeding `t` from `s`, or the first violation. -/
def run {ε : Type u} {σ : Type v} (m : Monitor ε σ) : σ → List ε → Except String σ
  | s, [] => .ok s
  | s, e :: t =>
    match m.step s e with
    | .ok s' => m.run s' t
    | .error reason => .error reason

/-- The monitor accepts trace `t` from its start. -/
def Accepts {ε : Type u} {σ : Type v} (m : Monitor ε σ) (t : List ε) : Prop :=
  ∃ s, m.run m.start t = .ok s

/-- The monitor accepts exactly the traces `p` allows. -/
def Sound {ε : Type u} {σ : Type v} (m : Monitor ε σ) (p : Protocol ε) : Prop :=
  ∀ t, m.Accepts t ↔ p.allowed t

/-- Checks each event with `ok`; `reason` explains a refused event. -/
def ofPredicate {ε : Type u} (ok : ε → Bool) (reason : ε → String) : Monitor ε Unit where
  start := ()
  step _ e := if ok e then .ok () else .error (reason e)

theorem run_cons_ok {ε : Type u} {σ : Type v} (m : Monitor ε σ) {s s' : σ} {e : ε} (t : List ε)
    (h : m.step s e = .ok s') : m.run s (e :: t) = m.run s' t := by
  simp [run, h]

theorem run_cons_error {ε : Type u} {σ : Type v} (m : Monitor ε σ) {s : σ} {e : ε} {r : String}
    (t : List ε) (h : m.step s e = .error r) : m.run s (e :: t) = .error r := by
  simp [run, h]

theorem ofPredicate_run {ε : Type u} (ok : ε → Bool) (reason : ε → String) :
    ∀ (t : List ε), (∃ s, (ofPredicate ok reason).run () t = .ok s) ↔ ∀ e ∈ t, ok e = true
  | [] => by simp [run]
  | e :: t => by
    by_cases h : ok e = true
    · have hs : (ofPredicate ok reason).step () e = .ok () := by simp [ofPredicate, h]
      rw [run_cons_ok _ t hs, ofPredicate_run ok reason t]
      simp [h]
    · have hs : (ofPredicate ok reason).step () e = .error (reason e) := by simp [ofPredicate, h]
      rw [run_cons_error _ t hs]
      simp [h]

theorem ofPredicate_sound {ε : Type u} (ok : ε → Bool) (reason : ε → String) :
    (ofPredicate ok reason).Sound (Protocol.always (fun e => ok e = true)) :=
  fun t => ofPredicate_run ok reason t

/-- Runs reducer `r`; `reason` explains an event `r` refuses in a state. -/
def ofReducer {ε : Type u} {σ : Type v} (r : Reducer ε σ) (reason : σ → ε → String) :
    Monitor ε σ where
  start := r.start
  step s e :=
    match r.step s e with
    | some s' => .ok s'
    | none => .error (reason s e)

theorem ofReducer_run {ε : Type u} {σ : Type v} (r : Reducer ε σ) (reason : σ → ε → String) :
    ∀ (t : List ε) (s : σ),
      (∃ s', (ofReducer r reason).run s t = .ok s') ↔ (t.foldlM r.step s).isSome
  | [], s => by simp [run]
  | e :: t, s => by
    simp only [run, ofReducer, List.foldlM_cons]
    cases h : r.step s e with
    | none => simp
    | some s' =>
      simp only
      exact ofReducer_run r reason t s'

theorem ofReducer_sound {ε : Type u} {σ : Type v} (r : Reducer ε σ) (reason : σ → ε → String) :
    (ofReducer r reason).Sound r.protocol :=
  fun t => ofReducer_run r reason t r.start

/-- Runs two monitors side by side; an event is accepted when both accept it, and the first
monitor's reason is reported when both refuse it. -/
def both {ε : Type u} {σ : Type v} {τ : Type w} (a : Monitor ε σ) (b : Monitor ε τ) :
    Monitor ε (σ × τ) where
  start := (a.start, b.start)
  step st e :=
    match a.step st.1 e, b.step st.2 e with
    | .ok x, .ok y => .ok (x, y)
    | .error r, _ => .error r
    | .ok _, .error r => .error r

theorem both_run {ε : Type u} {σ : Type v} {τ : Type w} (a : Monitor ε σ) (b : Monitor ε τ) :
    ∀ (t : List ε) (x : σ) (y : τ),
      (∃ s, (both a b).run (x, y) t = .ok s) ↔
        (∃ s, a.run x t = .ok s) ∧ (∃ s, b.run y t = .ok s)
  | [], x, y => by simp [run]
  | e :: t, x, y => by
    simp only [run, both]
    cases ha : a.step x e with
    | error r => simp
    | ok x' =>
      cases hb : b.step y e with
      | error r => simp
      | ok y' =>
        simp only
        exact both_run a b t x' y'

theorem both_sound {ε : Type u} {σ : Type v} {τ : Type w} {a : Monitor ε σ} {b : Monitor ε τ}
    {p q : Protocol ε} (ha : a.Sound p) (hb : b.Sound q) : (both a b).Sound (p.both q) := by
  intro t
  show (∃ s, (both a b).run (a.start, b.start) t = .ok s) ↔ p.allowed t ∧ q.allowed t
  rw [both_run]
  exact and_congr (ha t) (hb t)

end Monitor

end Lungo
