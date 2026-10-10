import Lungo

/-!
An async capability: operations the program asks the host to perform asynchronously, answered
with a string or an error, an opaque token, or a function.
-/
namespace Polyglot

/-- A token the program makes and the host passes back: a proof makes it opaque. -/
structure Token where
  n : Nat
  positive : n > 0

def mkToken (n : Nat) : Option Token :=
  if h : n > 0 then some ⟨n, h⟩ else none

def Token.value (t : Token) : Nat := t.n

inductive FetchOp where
  | get (url : String)
  | stamp (t : Token)
  | scaler (k : Nat)

@[lungo_capability "polyglot.fetch"]
instance fetchInterface : Lungo.Async.Interface FetchOp where
  Ret
    | .get _ => Except String String
    | .stamp _ => Token
    | .scaler _ => Nat → Nat

open Lungo.Async in
/-- Fetches every url, in order; a failed fetch is reported in place. -/
def fetchAll (urls : List String) : Program FetchOp (List String) := do
  let mut out := []
  for u in urls do
    match ← Program.perform (FetchOp.get u) with
    | .ok body => out := out ++ [body]
    | .error e => out := out ++ [s!"error: {e}"]
  return out

open Lungo.Async in
/-- Asks the host to stamp `t`, and returns the stamped token's value plus one. -/
def stampToken (t : Token) : Program FetchOp Nat := do
  let stamped ← Program.perform (FetchOp.stamp t)
  return stamped.value + 1

open Lungo.Async in
/-- Asks the host for a function and applies it to `x`. -/
def applyScaler (k x : Nat) : Program FetchOp Nat := do
  let f ← Program.perform (FetchOp.scaler k)
  return f x

end Polyglot
