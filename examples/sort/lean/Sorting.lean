import Lungo

/-!
Sorting with a contract: `sort` and `rank` are merge sorts, and the claims prove each returns a
sorted permutation of its input — whatever the input, in every language the crate is used from.
-/
namespace Sorting

open Lungo.Spec

/-- The contract of a sort by `le`: any input; the output is ordered by `le` and has exactly the
input's elements, as often as the input has them. -/
noncomputable def sortedPermutation {α : Type} (le : α → α → Bool) : Contract (List α) (List α) where
  pre _ := True
  post xs ys := ys.Pairwise (fun a b => le a b) ∧ ys.Perm xs

/-- Numbers in ascending order. -/
@[lungo_spec "lungo.contract"]
noncomputable def SortsAscending : Contract (List Nat) (List Nat) := sortedPermutation fun a b => decide (a ≤ b)

def sort (xs : List Nat) : List Nat := xs.mergeSort fun a b => decide (a ≤ b)

@[lungo_claim "lungo.satisfies" subject sort spec SortsAscending]
theorem sort_satisfies : Satisfies sort SortsAscending := by
  intro xs _
  refine ⟨List.pairwise_mergeSort ?_ ?_ xs, List.mergeSort_perm xs _⟩
  · intro a b c hab hbc
    simp only [decide_eq_true_eq] at *
    omega
  · intro a b
    simp only [Bool.or_eq_true, decide_eq_true_eq]
    omega

/-- A competitor's score. -/
structure Entry where
  name : String
  score : Nat
  deriving Repr, BEq

/-- `a` ranks no lower than `b`. -/
def ranksBefore (a b : Entry) : Bool := decide (b.score ≤ a.score)

/-- Entries from the highest score down. -/
@[lungo_spec "lungo.contract"]
noncomputable def RanksByScore : Contract (List Entry) (List Entry) := sortedPermutation ranksBefore

/-- The entries from the highest score down; entries with the same score keep their order. -/
def rank (es : List Entry) : List Entry := es.mergeSort ranksBefore

theorem ranksBefore_trans (a b c : Entry) : ranksBefore a b → ranksBefore b c → ranksBefore a c := by
  simp only [ranksBefore, decide_eq_true_eq]
  omega

theorem ranksBefore_total (a b : Entry) : (ranksBefore a b || ranksBefore b a) = true := by
  simp only [ranksBefore, Bool.or_eq_true, decide_eq_true_eq]
  omega

@[lungo_claim "lungo.satisfies" subject rank spec RanksByScore]
theorem rank_satisfies : Satisfies rank RanksByScore :=
  fun es _ => ⟨List.pairwise_mergeSort ranksBefore_trans ranksBefore_total es, List.mergeSort_perm es _⟩

/-- Ranking is stable: of two entries with the same score, the earlier stays earlier. -/
@[lungo_claim "lungo.law" subject rank]
theorem rank_stable (a b : Entry) (es : List Entry) (same : a.score = b.score) (h : [a, b].Sublist es) :
    [a, b].Sublist (rank es) :=
  List.pair_sublist_mergeSort ranksBefore_trans ranksBefore_total (by simp [ranksBefore, same]) h

end Sorting
