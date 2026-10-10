import Lungo

/-!
An account as a history of events. `apply` and `replay` are the executable reducer the crate
exports; `Solvent` is the protocol the history must follow, stated about histories alone: at no
point has more been withdrawn than deposited. The claims prove that every history `replay`
accepts is solvent, and that the balance it reports is what the history deposited less what it
withdrew.
-/
namespace Ledger

open Lungo.Trace

inductive Event where
  | deposit (amount : Nat)
  | withdraw (amount : Nat)
  deriving Repr, BEq

/-- The balance after `e`; `none` when `e` withdraws more than `balance`. -/
def apply (balance : Nat) (e : Event) : Option Nat :=
  match e with
  | .deposit n => some (balance + n)
  | .withdraw n => if n ≤ balance then some (balance - n) else none

/-- The account as a reducer: from nothing, by `apply`. -/
private def account : Reducer Event Nat := ⟨0, apply⟩

/-- The balance after `events`, from nothing; `none` when one of them overdraws. -/
def replay (events : List Event) : Option Nat := account.run events

private def depositOf : Event → Nat
  | .deposit n => n
  | .withdraw _ => 0

private def withdrawalOf : Event → Nat
  | .deposit _ => 0
  | .withdraw n => n

/-- What `events` deposited. -/
def deposited (events : List Event) : Nat := (events.map depositOf).sum

/-- What `events` withdrew. -/
def withdrawn (events : List Event) : Nat := (events.map withdrawalOf).sum

theorem take_of_append (t : List Event) (e : Event) (k : Nat) :
    t.take k = (t ++ [e]).take (min k t.length) := by
  rw [List.take_append_of_le_length (by omega)]
  by_cases h : k ≤ t.length
  · simp [Nat.min_eq_left h]
  · rw [Nat.min_eq_right (by omega), List.take_length, List.take_of_length_le (by omega)]

/-- The histories in which nothing was ever withdrawn that had not been deposited. -/
@[lungo_spec "lungo.protocol"]
noncomputable def Solvent : Protocol Event where
  allowed t := ∀ k, withdrawn (t.take k) ≤ deposited (t.take k)
  nil_allowed := by simp [withdrawn, deposited]
  prefix_closed t e h k := by
    rw [take_of_append t e k]
    exact h _

theorem account_refines : ReducerRefines account Solvent := by
  refine refines_of_invariant account Solvent
    (fun t balance => balance + withdrawn t = deposited t ∧ Solvent.allowed t)
    ⟨by simp [account, withdrawn, deposited], Solvent.nil_allowed⟩ ?_ (fun _ _ h => h.2)
  intro t balance e balance' ⟨hb, ht⟩ hstep
  have now : withdrawn t ≤ deposited t := by omega
  have grows : ∀ k, (t ++ [e]).take k = t.take k ∨ (t ++ [e]).take k = t ++ [e] := by
    intro k
    by_cases hk : k ≤ t.length
    · left; exact (List.take_append_of_le_length hk).trans (by simp)
    · right; exact List.take_of_length_le (by simp; omega)
  cases e with
  | deposit n =>
    simp only [account, apply, Option.some.injEq] at hstep
    subst hstep
    refine ⟨by simp [withdrawn, deposited, depositOf, withdrawalOf] at hb ⊢; omega, fun k => ?_⟩
    rcases grows k with h | h <;> rw [h]
    · exact ht k
    · simp [withdrawn, deposited, depositOf, withdrawalOf] at now ⊢; omega
  | withdraw n =>
    simp only [account, apply] at hstep
    split at hstep
    · rename_i enough
      simp only [Option.some.injEq] at hstep
      subst hstep
      refine ⟨by simp [withdrawn, deposited, depositOf, withdrawalOf] at hb ⊢; omega, fun k => ?_⟩
      rcases grows k with h | h <;> rw [h]
      · exact ht k
      · simp [withdrawn, deposited, depositOf, withdrawalOf] at hb ⊢; omega
    · cases hstep

/-- From any balance, applying `events` adds what they deposit and takes what they withdraw. -/
theorem run_balance : ∀ (events : List Event) (start balance : Nat),
    events.foldlM apply start = some balance → balance + withdrawn events = start + deposited events
  | [], start, balance, h => by
    simp only [List.foldlM_nil, Option.pure_def, Option.some.injEq] at h
    simp [h, withdrawn, deposited]
  | e :: rest, start, balance, h => by
    simp only [List.foldlM_cons, Option.bind_eq_bind] at h
    cases hstep : apply start e with
    | none => rw [hstep] at h; cases h
    | some mid =>
      rw [hstep] at h
      have ih := run_balance rest mid balance h
      cases e with
      | deposit n =>
        simp only [apply, Option.some.injEq] at hstep
        simp [withdrawn, deposited, depositOf, withdrawalOf] at ih ⊢
        omega
      | withdraw n =>
        simp only [apply] at hstep
        split at hstep
        · simp only [Option.some.injEq] at hstep
          simp [withdrawn, deposited, depositOf, withdrawalOf] at ih ⊢
          omega
        · cases hstep

/-- Every history `replay` accepts is solvent. -/
@[lungo_claim "lungo.law" subject replay spec Solvent]
theorem replay_solvent (events : List Event) (accepted : (replay events).isSome) :
    Solvent.allowed events :=
  account_refines events accepted

/-- The balance `replay` reports is what was deposited less what was withdrawn. -/
@[lungo_claim "lungo.law" subject replay]
theorem replay_balance (events : List Event) (balance : Nat) (h : replay events = some balance) :
    balance = deposited events - withdrawn events := by
  have := run_balance events 0 balance h
  omega

end Ledger
