import Lungo

/-!
Lean code asking the Rust application to perform operations asynchronously: the Rust facade
makes each export an `async fn` taking a handler.
-/
namespace Host

/-- What a job asks of the application. -/
inductive Ask where
  | fetch (key : String)
  | tool (name : String)

/-- A fetch answers a value or why there is none; a tool is a function of the application's. -/
@[lungo_facility "test.ask"]
instance askInterface : Lungo.Async.Interface Ask where
  Ret
    | .fetch _ => Except String Nat
    | .tool _ => Nat → Nat

open Lungo.Async in
/-- The total of the values fetched for `keys`, skipping those there are none for. -/
def sumKeys (keys : List String) : Program Ask Nat := do
  let mut total := 0
  for k in keys do
    match ← Program.perform (Ask.fetch k) with
    | .ok v => total := total + v
    | .error _ => pure ()
  return total

open Lungo.Async in
/-- Asks for the tool `name`, then fetches `key` and applies the tool to its value. -/
def useTool (name key : String) : Program Ask Nat := do
  let f ← Program.perform (Ask.tool name)
  match ← Program.perform (Ask.fetch key) with
  | .ok v => return f v
  | .error _ => return 0

end Host
