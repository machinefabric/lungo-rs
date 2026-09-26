/-! Custom syntax, notation, and macros, elaborated by Lean and compiled like any other code. -/

declare_syntax_cat arith
syntax num : arith
syntax arith "⊕" arith : arith
syntax "(" arith ")" : arith
syntax "[arith|" arith "]" : term

macro_rules
  | `([arith| $n:num]) => `($n)
  | `([arith| $a ⊕ $b]) => `([arith| $a] * 2 + [arith| $b])
  | `([arith| ($a)]) => `([arith| $a])

notation:65 a " ⊞ " b => a * a + b * b

macro "repeat_str" s:str n:num : term => `(String.join (List.replicate $n $s))

def main : IO Unit := do
  IO.println ([arith| 1 ⊕ (2 ⊕ 3)] : Nat)
  IO.println ((3 : Nat) ⊞ 4)
  IO.println (repeat_str "ab" 3)
