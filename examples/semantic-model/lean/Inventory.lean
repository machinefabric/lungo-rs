import Lungo
import Acme

/-!
An inventory whose lookups are proved to cost what Acme's model allows: a claim under Acme's own
relation, `acme.cost-bound`, about Acme's specification `Acme.Linear`.
-/
namespace Inventory

structure Item where
  sku : String
  count : Nat
  deriving Repr, BEq

/-- The count of `sku`, and the steps the lookup took: one per item examined, and one more. -/
def lookupCounted (sku : String) : List Item → Option Nat × Nat
  | [] => (none, 1)
  | item :: rest =>
    if item.sku == sku then (some item.count, 1)
    else
      let (found, steps) := lookupCounted sku rest
      (found, steps + 1)

/-- The count of `sku`, if the inventory has it. -/
def lookup (sku : String) (items : List Item) : Option Nat := (lookupCounted sku items).1

/-- The steps looking `sku` up takes. -/
def lookupSteps (sku : String) (items : List Item) : Nat := (lookupCounted sku items).2

@[lungo_claim "lungo.equals" subject lookup]
theorem lookup_eq (sku : String) (items : List Item) :
    lookup sku items = (items.find? (·.sku == sku)).map (·.count) := by
  induction items with
  | nil => rfl
  | cons item rest ih =>
    by_cases h : item.sku = sku
    · simp [lookup, lookupCounted, h]
    · simp only [lookup] at ih
      simp [lookup, lookupCounted, h, ih]

/-- A lookup examines each item at most once: linear in the inventory's size. -/
@[lungo_claim "acme.cost-bound" subject lookupSteps spec Acme.Linear]
theorem lookup_linear (sku : String) : Acme.Linear List.length 1 (lookupSteps sku) := by
  intro items
  induction items with
  | nil => simp [lookupSteps, lookupCounted]
  | cons item rest ih =>
    simp only [lookupSteps, lookupCounted] at ih ⊢
    split <;> simp_all <;> omega

end Inventory
