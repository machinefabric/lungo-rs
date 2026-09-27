/-!
A program whose types another program uses: `Pos` carries a proof, so bindings hold its values
by handle; `Pair` is plain data.
-/
namespace Provider

/-- A positive number: made only by `mkPos`, which checks. -/
structure Pos where
  n : Nat
  pos : 0 < n

/-- Plain data. -/
structure Pair where
  count : Nat
  label : String

def mkPos (n : Nat) : Option Pos := if h : 0 < n then some ⟨n, h⟩ else none

def value (p : Pos) : Nat := p.n

def makePair (count : Nat) (label : String) : Pair := ⟨count, label⟩

end Provider
