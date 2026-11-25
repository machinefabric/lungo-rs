/-!
The types every language binding represents: a structure with scalar and object fields, an
inductive with several constructors, and a polymorphic recursive type. Some declarations carry
docstrings and some do not, as the bindings' documentation reads them.
-/
namespace Polyglot

structure Point where
  /-- The distance from the left edge,
  in pixels. -/
  x : Float
  y : Float
  /-- What the point is called. -/
  label : String
  tag : UInt8
deriving Repr, BEq

inductive Shape where
  /-- A disc around `center`. -/
  | circle (center : Point) (radius : Float)
  | rect (corner : Point) (width : Float) (height : Float)
  | empty
deriving Repr

/-- What looking a slot up answers. Named after the operation that returns it, as Lean code
is written: the type and the function are two names in every binding. -/
inductive Lookup where
  | found (slot : Nat)
  | missing
deriving Repr

inductive Tree (α : Type) where
  | leaf
  | node (left : Tree α) (value : α) (right : Tree α)
deriving Repr

end Polyglot
