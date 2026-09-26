/-! `partial`, `unsafe`, and `implemented_by` definitions. -/

partial def collatz (n : Nat) (steps : Nat := 0) : Nat :=
  if n ≤ 1 then steps else if n % 2 == 0 then collatz (n / 2) (steps + 1) else collatz (3 * n + 1) (steps + 1)

unsafe def samePtrUnsafe (a b : String) : Bool := ptrAddrUnsafe a == ptrAddrUnsafe b

@[implemented_by samePtrUnsafe]
def samePtr (a b : String) : Bool := a == b

def sumTo (n : Nat) : Nat := Id.run do
  let mut acc := 0
  for i in [0:n+1] do acc := acc + i
  return acc

def main : IO Unit := do
  IO.println ((List.range 12).map (collatz ·))
  IO.println (collatz 837799)
  let s := "shared"
  IO.println (samePtr s s)
  IO.println (sumTo 1000000)
