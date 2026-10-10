import Lungo

/-!
The facilities the application provides in the host language: a scaler and a journal, each an
`@[extern]` operation the host implements.
-/
namespace Polyglot

/-- Scales numbers by the host's factor. -/
@[lungo_facility "polyglot.scaler"]
structure Scaler

/-- Keeps the host's log. -/
@[lungo_facility "polyglot.journal"]
structure Journal

/-- Scales by the host's factor (pure). -/
@[extern "polyglot_scale", lungo_operation Scaler]
opaque hostScale (n : Nat) : Nat

/-- Records a line in the host's log; may fail. -/
@[extern "polyglot_record", lungo_operation Journal]
opaque hostRecord (line : String) : IO Unit

/-- What the program's claims assume of the host's scaler, and cannot prove: it scales a larger
number to a larger one. -/
@[lungo_assumption Scaler]
def ScalesMonotonically : Prop := ∀ a b, a ≤ b → hostScale a ≤ hostScale b

def scaledSum (xs : List Nat) : Nat := xs.foldl (fun acc x => acc + hostScale x) 0

/-- Records every line through the host, stopping at the first failure. -/
def recordAll (lines : List String) : IO Nat := do
  for l in lines do
    hostRecord l
  return lines.length

end Polyglot
