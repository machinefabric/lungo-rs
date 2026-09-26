import Shapes.Circle
import Shapes.Square

def Tool.report (n : Nat) : String :=
  s!"{Shapes.Circle.diameter n} {Shapes.Square.area n}"

def main (args : List String) : IO UInt32 := do
  IO.println (Tool.report args.length)
  return 0
