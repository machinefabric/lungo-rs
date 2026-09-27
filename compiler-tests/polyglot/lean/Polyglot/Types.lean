/-!
The types every language binding represents: a structure with scalar and object fields, an
inductive with several constructors, and a polymorphic recursive type.
-/
namespace Polyglot

structure Point where
  x : Float
  y : Float
  label : String
  tag : UInt8
deriving Repr, BEq

inductive Shape where
  | circle (center : Point) (radius : Float)
  | rect (corner : Point) (width : Float) (height : Float)
  | empty
deriving Repr

inductive Tree (α : Type) where
  | leaf
  | node (left : Tree α) (value : α) (right : Tree α)
deriving Repr

end Polyglot
