/-! Inductive types, structures, recursion, pattern matching, and derived instances. -/

inductive Tree (α : Type) where
  | leaf
  | node (left : Tree α) (value : α) (right : Tree α)
  deriving Repr, BEq

namespace Tree

def insert [Ord α] (t : Tree α) (x : α) : Tree α :=
  match t with
  | leaf => node leaf x leaf
  | node l v r =>
    match compare x v with
    | .lt => node (l.insert x) v r
    | .gt => node l v (r.insert x)
    | .eq => t

def toList : Tree α → List α
  | leaf => []
  | node l v r => l.toList ++ [v] ++ r.toList

def depth : Tree α → Nat
  | leaf => 0
  | node l _ r => max l.depth r.depth + 1

def size : Tree α → Nat
  | leaf => 0
  | node l _ r => l.size + 1 + r.size

end Tree

structure Point where
  x : Int
  y : Int
  deriving Repr, BEq, Hashable, Ord

structure Particle where
  pos : Point
  mass : Float
  tag : UInt8
  alive : Bool
  name : String
  deriving Repr

inductive Shape where
  | circle (r : Nat)
  | rect (w h : Nat)
  | poly (pts : List Point)
  | empty
  deriving Repr, BEq

def area : Shape → Nat
  | .circle r => 3 * r * r
  | .rect w h => w * h
  | .poly pts => pts.length
  | .empty => 0

inductive Expr where
  | num (n : Int)
  | var (name : String)
  | add (a b : Expr)
  | mul (a b : Expr)
  | neg (a : Expr)
  deriving Repr, Inhabited

def Expr.eval (env : String → Int) : Expr → Int
  | num n => n
  | var x => env x
  | add a b => a.eval env + b.eval env
  | mul a b => a.eval env * b.eval env
  | neg a => - a.eval env

def Expr.simp : Expr → Expr
  | add (num 0) e | add e (num 0) => e.simp
  | mul (num 1) e | mul e (num 1) => e.simp
  | mul (num 0) _ | mul _ (num 0) => num 0
  | add a b => add a.simp b.simp
  | mul a b => mul a.simp b.simp
  | neg (neg e) => e.simp
  | neg e => neg e.simp
  | e => e

mutual
def isEven : Nat → Bool
  | 0 => true
  | n + 1 => isOdd n
def isOdd : Nat → Bool
  | 0 => false
  | n + 1 => isEven n
end

def ackermann : Nat → Nat → Nat
  | 0, n => n + 1
  | m + 1, 0 => ackermann m 1
  | m + 1, n + 1 => ackermann m (ackermann (m + 1) n)
termination_by m n => (m, n)

/-- A structure with `USize`, other scalar, and object fields (`uproj`/`uset`, `sproj`/`sset`). -/
structure Cursor where
  pos : USize
  line : UInt32
  col : UInt16
  text : String
  deriving Repr

def Cursor.advance (c : Cursor) (ch : Char) : Cursor :=
  if ch == '\n' then { c with pos := c.pos + 1, line := c.line + 1, col := 0 }
  else { c with pos := c.pos + 1, col := c.col + 1, text := c.text.push ch }

def scan (s : String) : Cursor := s.foldl Cursor.advance ⟨0, 1, 0, ""⟩

def main : IO Unit := do
  let t := [5, 3, 8, 1, 4, 7, 9, 2, 6, 5, 3].foldl Tree.insert Tree.leaf
  IO.println s!"{t.toList} depth={t.depth} size={t.size}"
  IO.println (repr ((Tree.leaf : Tree Nat).insert 1))
  IO.println (t == t.insert 5, t == t.insert 10)
  let p := Particle.mk ⟨3, -4⟩ 1.5 200 true "proton"
  IO.println (repr p)
  IO.println (repr { p with pos := { p.pos with x := 10 }, alive := false })
  IO.println (hash (Point.mk 1 2) == hash (Point.mk 1 2), repr (compare (Point.mk 1 2) (Point.mk 1 3)))
  IO.println (hash (Point.mk 1 2), hash (Point.mk (-7) 123456789012345678901234567890), hash [Point.mk 0 0])
  let shapes := [Shape.circle 2, .rect 3 4, .poly [⟨0, 0⟩, ⟨1, 1⟩], .empty]
  IO.println (shapes.map area, shapes.map repr)
  let e := Expr.add (Expr.mul (Expr.num 1) (Expr.var "x")) (Expr.neg (Expr.neg (Expr.num 0)))
  IO.println (repr e.simp, e.eval (fun _ => 21), (Expr.mul (.var "y") (.num 0)).simp |> repr)
  IO.println (isEven 1001, isOdd 1001, ackermann 2 3, ackermann 3 3)
  let big := (List.range 3000).foldl Tree.insert Tree.leaf
  IO.println s!"big size={big.size} depth={big.depth}"
  let c := scan "ab\ncd\nef"
  IO.println (repr c, c.pos.toNat, (scan "").pos.toNat)
