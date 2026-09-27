import Provider

/-!
A program built on `provider`'s model: its functions take and return `provider`'s types, which
bindings take from `provider`'s package rather than generating their own.
-/
namespace Consumer

open Provider

/-- Twice `p`, still positive. -/
def double (p : Pos) : Pos := ⟨p.n * 2, by have := p.pos; omega⟩

/-- `p` counted in `q`. -/
def count (p : Pos) (q : Pair) : Pair := ⟨q.count + p.n, q.label⟩

def describe (q : Pair) : String := s!"{q.label}: {q.count}"

end Consumer
