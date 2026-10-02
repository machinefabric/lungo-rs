import Polyglot.Types
/-!
Exported functions over every kind of value crossing the boundary.
-/
namespace Polyglot

/-- Natural numbers of any size. -/
def factorial : Nat → Nat
  | 0 => 1
  | n + 1 => (n + 1) * factorial n

/-- Negative and large integers. -/
def negate (i : Int) : Int := -i

def area : Shape → Float
  | .circle _ r => 3.0 * r * r
  | .rect _ w h => w * h
  | .empty => 0

/-- A registry of `size` slots: the slot asked for is found when there is one. Named like the
type it returns (`Lookup`); where a language writes functions and types in one case, the
function keeps one more component (`RegistryLookup` in Go). -/
def Registry.lookup (size slot : Nat) : Lookup :=
  if slot < size then .found slot else .missing

def moveBy (p : Point) (dx dy : Float) : Point := { p with x := p.x + dx, y := p.y + dy }

def describe (p : Point) : String := s!"{p.label}#{p.tag} at ({p.x}, {p.y})"

/-- Fixed-width integers and characters. -/
def mix (a : UInt64) (b : Int32) (c : Char) (d : Bool) : String :=
  s!"{a}/{b}/{c}/{d}"

def reverseBytes (b : ByteArray) : ByteArray := ⟨b.data.reverse⟩

def sumFloats (xs : FloatArray) : Float := xs.foldl (· + ·) 0

def firstWord (s : String) : Option String :=
  (s.splitOn " ").head?.filter (!·.isEmpty)

def divide (a b : Nat) : Except String Nat :=
  if b == 0 then .error "division by zero" else .ok (a / b)

def swap (p : String × Nat) : Nat × String := (p.2, p.1)

def evens (n : Nat) : Array Nat := (List.range n).filter (· % 2 == 0) |>.toArray

/-- Polymorphic: the binding passes the element type. -/
def Tree.size {α : Type} : Tree α → Nat
  | .leaf => 0
  | .node l _ r => l.size + 1 + r.size

def Tree.mirror {α : Type} : Tree α → Tree α
  | .leaf => .leaf
  | .node l v r => .node r.mirror v l.mirror

def Tree.toList {α : Type} : Tree α → List α
  | .leaf => []
  | .node l v r => l.toList ++ [v] ++ r.toList

def Tree.ofList {α : Type} : List α → Tree α
  | [] => .leaf
  | x :: xs => .node .leaf x (Tree.ofList xs)

/-- A function from the host, called by Lean. -/
def applyTwice (f : Nat → Nat) (x : Nat) : Nat := f (f x)

/-- A Lean closure, called by the host. -/
def adder (k : Nat) : Nat → Nat := fun x => x + k

/-- IO that fails with an `IO.Error`. -/
def checkedDiv (a b : Nat) : IO Nat := do
  if b == 0 then throw (IO.userError "checkedDiv: division by zero")
  return a / b

/-- EIO with a typed error. -/
def parseDigit (c : Char) : EIO String Nat :=
  if c.isDigit then pure (c.toNat - '0'.toNat) else throw s!"not a digit: {c}"

/-- An opaque value: a mutable reference the host holds by handle. -/
def newCounter (start : Nat) : IO (IO.Ref Nat) := IO.mkRef start

def bump (r : IO.Ref Nat) : IO Nat := do
  r.modify (· + 1)
  r.get

end Polyglot
