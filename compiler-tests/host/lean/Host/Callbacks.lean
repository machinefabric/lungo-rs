/-!
Lean code calling back into the Rust application through `@[extern]` declarations that the
application maps with `Config::rust_extern`.
-/
namespace Host

/-- Looks a key up in the host's table. -/
@[extern "host_lookup"]
opaque hostLookup (key : @& String) : Option Nat

/-- Appends a line to the host's log. -/
@[extern "host_log"]
opaque hostLog (line : String) : IO Unit

/-- Calls a Lean function provided by the host (a closure crossing the boundary twice). -/
@[extern "host_transform"]
opaque hostTransform (f : Nat → Nat) (xs : List Nat) : List Nat

/-- Resolves `keys` through the host, logging each resolution; returns the total of the
found values, or an error naming the first missing key. -/
def resolveAll (keys : List String) : IO Nat := do
  let mut total := 0
  for k in keys do
    match hostLookup k with
    | some v =>
      hostLog s!"{k} => {v}"
      total := total + v
    | none => throw (IO.userError s!"unknown key: {k}")
  return total

def transformed (xs : List Nat) (k : Nat) : List Nat :=
  hostTransform (fun x => x * k + 1) xs

end Host
