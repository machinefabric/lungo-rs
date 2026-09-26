module

public def identity (n : Nat) : Nat := n

public theorem incorrect : identity 0 = 1 := by decide
