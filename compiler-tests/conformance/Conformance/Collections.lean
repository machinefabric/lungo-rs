import Std.Data.HashMap
import Std.Data.HashSet
/-! Arrays (in-place updates), byte and float arrays, lists, and Std hash maps. -/

open Std

def sieve (n : Nat) : Array Nat := Id.run do
  let mut marks := Array.replicate (n + 1) true
  let mut primes := #[]
  for i in [2:n+1] do
    if marks[i]! then
      primes := primes.push i
      let mut j := i * i
      while j ≤ n do
        marks := marks.set! j false
        j := j + i
  return primes

def histogram (xs : List String) : HashMap String Nat :=
  xs.foldl (fun m w => m.insert w (m.getD w 0 + 1)) {}

def main : IO Unit := do
  let ps := sieve 200
  IO.println s!"{ps.size} {ps}"
  let mut a : Array Nat := #[]
  for i in [0:20] do a := a.push (i * i)
  a := a.set! 3 1000
  a := a.swapIfInBounds 0 19
  IO.println (a, a.pop, a.back?, a[100]?, a.reverse, a.extract 2 6, a.foldl (· + ·) 0)
  IO.println (a.contains 1000, a.any (· > 300), a.all (· < 400), a.findIdx? (· == 49))
  IO.println (#[3, 1, 2].insertionSort (· < ·), #[1, 2, 3].zip #["a", "b", "c"])
  let bs := ByteArray.mk #[0, 1, 2, 255]
  let bs := bs.push 128 |>.set! 1 77
  IO.println (bs.toList, bs.size, bs.get! 3, (bs.extract 1 3).toList, bs.data.size)
  let fa := FloatArray.mk #[1.5, 2.25, -3.0]
  IO.println (fa.push 4.0 |>.toList, fa.get! 1, fa.size)
  let words := "the quick brown fox jumps over the lazy dog the end".splitOn " "
  let h := histogram words
  IO.println ((words.eraseDups.map fun w => (w, h.getD w 0)))
  let s : HashSet Nat := (List.range 50).foldl (fun s i => s.insert (i * 7 % 13)) {}
  IO.println (s.size, s.contains 12, s.contains 13)
  IO.println ([1, 2, 3, 4].map (· * 2) |>.reverse |>.take 3, [3, 1, 2].mergeSort, List.replicate 3 'x')
  IO.println ((List.range 10).toArray.filter (· % 2 == 1) |>.map (· * 3))
  let nested := #[#[1, 2], #[3], #[]]
  IO.println (nested.flatten, nested.map Array.size)
  let big := (List.range 100000).toArray
  IO.println (big.foldl (· + ·) 0, (big.map (· + 1)).size)
