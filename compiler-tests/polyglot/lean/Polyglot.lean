import Polyglot.Types
import Polyglot.Functions
import Polyglot.Host

/-- The program's entry point. -/
def main (args : List String) : IO UInt32 := do
  IO.println s!"polyglot {args}"
  IO.println s!"20! = {Polyglot.factorial 20}"
  return (UInt32.ofNat args.length)
