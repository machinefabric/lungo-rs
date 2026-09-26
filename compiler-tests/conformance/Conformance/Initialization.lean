/-! Module initializers run once, before `main`. -/

initialize counterRef : IO.Ref Nat ← IO.mkRef 40

initialize
  counterRef.modify (· + 2)

builtin_initialize greeting : String ← pure "initialized"

def main : IO Unit := do
  IO.println s!"{greeting} {← counterRef.get}"
  IO.println (← IO.initializing)
