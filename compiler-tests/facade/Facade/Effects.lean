namespace Facade

def countTo (n : Nat) : IO Nat := do
  let r ← IO.mkRef 0
  for i in [0:n] do r.modify (· + i)
  r.get

def failIfOdd (n : Nat) : IO String := do
  if n % 2 == 1 then throw (IO.userError s!"odd: {n}")
  return s!"even: {n}"

def pureEffect (s : String) : BaseIO Nat := pure s.length

end Facade
