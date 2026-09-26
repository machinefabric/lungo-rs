/-!
Custom syntax, macros and derived instances: the project is built by Lake unchanged, and the
bridge only sees the compiled result.
-/
namespace Host

/-- `sum! a, b, c` adds its arguments. -/
syntax "sum! " term,+ : term

macro_rules
  | `(sum! $x) => `($x)
  | `(sum! $x, $xs,*) => `($x + sum! $xs,*)

/-- `x |>> f` applies `f` twice. -/
notation:60 x " |>> " f => f (f x)

/-- A tiny command-level macro generating a definition. -/
macro "defconst " n:ident " := " v:term : command => `(def $n : Nat := $v)

defconst answer := sum! 20, 20, 2

inductive Token where
  | num (n : Nat)
  | plus
  | times
  deriving Repr, BEq, Hashable, Inhabited

/-- Evaluates a token stream in which `times` binds tighter than `plus`. -/
def evalTokens (ts : List Token) : Nat :=
  let rec go (ts : List Token) (sum prod : Nat) : Nat :=
    match ts with
    | [] => sum + prod
    | .num n :: rest => go rest sum (prod * n)
    | .times :: rest => go rest sum prod
    | .plus :: rest => go rest (sum + prod) 1
  go ts 0 1

def macroDemo (n : Nat) : Nat := sum! n, answer, (n |>> (· * 3))

def tokenHash (t : Token) : UInt64 := hash t

def reprToken (t : Token) : String := toString (repr t)

end Host
