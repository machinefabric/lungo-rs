/-! Panics report and continue with the default value; `main` sets the exit code. -/

def safeHead (xs : List Nat) : Nat := xs.head!

def main (args : List String) : IO UInt32 := do
  IO.println s!"args: {args}"
  IO.println (safeHead [])
  IO.println (#[1, 2, 3][7]!)
  IO.println ((panic! "custom panic" : String) ++ "|")
  IO.println ("abc".toList[10]!)
  IO.println "after panics"
  return 3
