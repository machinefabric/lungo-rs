namespace Facade

inductive Color where
  | red | green | blue
  deriving Repr, BEq

structure Pixel where
  x : UInt16
  y : UInt16
  color : Color
  alpha : Float
  label : String
  deriving Repr

structure Meters where
  value : Nat

inductive Tree (α : Type) where
  | leaf
  | node (left : Tree α) (value : α) (right : Tree α)

def Tree.insert [Ord α] : Tree α → α → Tree α
  | .leaf, x => .node .leaf x .leaf
  | .node l v r, x =>
    match compare x v with
    | .lt => .node (l.insert x) v r
    | .gt => .node l v (r.insert x)
    | .eq => .node l v r

def Tree.toList : Tree α → List α
  | .leaf => []
  | .node l v r => l.toList ++ [v] ++ r.toList

def buildTree (xs : List Int) : Tree Int := xs.foldl Tree.insert .leaf

def treeToList (t : Tree Int) : List Int := t.toList

def mirror : Tree α → Tree α
  | .leaf => .leaf
  | .node l v r => .node (mirror r) v (mirror l)

def recolor (p : Pixel) (c : Color) : Pixel := { p with color := c, alpha := p.alpha / 2 }

def brighten (ps : Array Pixel) : Array Pixel := ps.map fun p => { p with x := p.x + 1, label := p.label ++ "!" }

def addMeters (a b : Meters) : Meters := ⟨a.value + b.value⟩

inductive Shape where
  | circle (radius : Nat)
  | rect (width height : Nat)
  | tagged (name : String) (inner : Shape)
  | none

def Shape.area : Shape → Nat
  | .circle r => 3 * r * r
  | .rect w h => w * h
  | .tagged _ s => s.area
  | .none => 0

def describeShape (s : Shape) : String × Nat := (toString s.area, s.area * 2)

def swapPair (p : Nat × String) : String × Nat := (p.2, p.1)

def classify (x : Int) : Except String Nat :=
  if x < 0 then .error s!"negative: {x}" else .ok x.toNat

def firstSome (xs : List (Option Nat)) : Option Nat := xs.findSome? id

end Facade
