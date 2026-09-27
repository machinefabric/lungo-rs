/-!
Externs the application implements in the host language.
-/
namespace Polyglot

/-- Scales by the host's factor (pure). -/
@[extern "polyglot_scale"]
opaque hostScale (n : Nat) : Nat

/-- Records a line in the host's log; may fail. -/
@[extern "polyglot_record"]
opaque hostRecord (line : String) : IO Unit

def scaledSum (xs : List Nat) : Nat := xs.foldl (fun acc x => acc + hostScale x) 0

/-- Records every line through the host, stopping at the first failure. -/
def recordAll (lines : List String) : IO Nat := do
  for l in lines do
    hostRecord l
  return lines.length

end Polyglot
