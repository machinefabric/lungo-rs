import Lungo

/-!
A session: opened, ticked and closed. `apply` and `run` are the executable state machine the
crate exports; `Spec` says, without computing anything, what each operation must do, and the
claims prove `step` meets it and what `run` does with ticks.
-/
namespace Formal

open Lungo.Spec

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

/-- One step of the session: whether `op` was accepted, and the session after it (unchanged when
it was not). -/
def step (s : Sess) (op : Op) : Bool × Sess :=
  match apply op s with
  | some s' => (true, s')
  | none => (false, s)

/-- What a session may do. A session starts closed with no ticks. Opening is accepted only when
closed and closing only when open, each flipping `isOpen` and keeping the count; a tick is always
accepted and counts one more, open or not. A rejected operation leaves the session as it was. -/
@[lungo_spec "lungo.state"]
noncomputable def Spec : StateSpec Sess Op Bool where
  init s := s = { isOpen := false, count := 0 }
  invariant _ := True
  step s op accepted s' :=
    match op with
    | .«open» => (accepted = !s.isOpen) ∧ s' = (if accepted then { s with isOpen := true } else s)
    | .close => (accepted = s.isOpen) ∧ s' = (if accepted then { s with isOpen := false } else s)
    | .tick => accepted = true ∧ s' = { s with count := s.count + 1 }

@[lungo_claim "lungo.preserves" subject step spec Spec]
theorem step_implements : Spec.Implements step := by
  intro s op _
  cases op <;> cases h : s.isOpen <;> simp [Spec, step, apply, h]

/-- Applies a sequence of operations, stopping at the first rejected one. -/
def run : List Op → Sess → Option Sess
  | [], s => some s
  | op :: ops, s => (apply op s).bind (run ops)

/-- Ticks are never rejected: `n` of them count `n` more, open or not. -/
@[lungo_claim "lungo.law" subject run]
theorem run_ticks (n : Nat) (s : Sess) :
    run (List.replicate n .tick) s = some { s with count := s.count + n } := by
  induction n generalizing s with
  | zero => simp [run]
  | succ n ih => simp [List.replicate, run, apply, ih, Nat.add_assoc, Nat.add_comm 1 n]

end Formal
