/-! Closures, partial application, over-application, and functions of many arguments. -/

def compose (fs : List (Nat → Nat)) : Nat → Nat :=
  fs.foldr (· ∘ ·) id

def adder (n : Nat) : Nat → Nat := fun x => x + n

def curried (a b c d e f g h i j k l m n o p q r s : Nat) : Nat :=
  a + 2*b + 3*c + 4*d + 5*e + 6*f + 7*g + 8*h + 9*i + 10*j + 11*k + 12*l + 13*m + 14*n + 15*o +
  16*p + 17*q + 18*r + 19*s

def applyN (f : α → α) : Nat → α → α
  | 0, x => x
  | n + 1, x => applyN f n (f x)

def twice (f : α → α) : α → α := f ∘ f

def counter : StateM Nat Nat := do
  modify (· + 1)
  let s ← get
  set (s * 2)
  return s

def pipeline : List Int → List Int :=
  List.filter (· % 3 ≠ 0) ∘ List.map (· * 7 - 50) ∘ List.reverse

def main : IO Unit := do
  IO.println ((compose [adder 1, (· * 3), adder 10]) 5)
  let partials := (List.range 5).map adder
  IO.println (partials.map (· 100))
  let f := curried 1 2 3
  let g := f 4 5 6 7 8 9 10
  let h := g 11 12 13 14 15 16 17 18
  IO.println (h 19, curried 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1 1)
  let fs : List (Nat → Nat → Nat → Nat) := [fun a b c => a + b + c, fun a b c => a * b * c, fun a _ c => a - c]
  IO.println (fs.map (fun k => k 7 5 3))
  IO.println (applyN (twice (· + 3)) 10 0, applyN (fun s => s ++ "!") 3 "hi")
  IO.println (counter.run 20)
  IO.println (pipeline (List.range 12 |>.map Int.ofNat))
  let table := (List.range 4).map fun i => (List.range 4).map fun j => (fun (x y : Nat) => x * 10 + y) i j
  IO.println table
  let sorted := #[5, 2, 9, 1, 5, 6, 0, 3].qsort (· < ·)
  IO.println sorted
  IO.println ((List.range 10).filterMap (fun x => if x % 2 == 0 then some (x * x) else none))
  IO.println (List.zipWith (· + ·) [1, 2, 3] [10, 20, 30, 40])
  IO.println ((List.range 6).partition (· < 3), (List.range 6).span (· < 3))
  let fns := #[Nat.succ, (· * 2), Nat.pred, fun x => x ^ 2]
  IO.println (fns.map (· 7))
