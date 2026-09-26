# How to call Rust functions from Lean

This guide shows how to let Lean code call a function of your Rust application: declare it
in Lean as an `@[extern]` constant, implement it in Rust, and map one to the other.

## Declare the function in Lean

Declare an opaque constant with the symbol the Rust side will implement:

```lean
namespace Host

/-- Looks a key up in the host's table. -/
@[extern "host_lookup"]
opaque hostLookup (key : @& String) : Option Nat

/-- Appends a line to the host's log. -/
@[extern "host_log"]
opaque hostLog (line : String) : IO Unit

end Host
```

The result type needs an `Inhabited` instance (Lean requires one for `opaque`). `@&` marks a
parameter Lean lends rather than transfers; the Rust function receives an owned Rust value
either way.

Use it from Lean like any other function:

```lean
def resolve (k : String) : IO Nat := do
  match Host.hostLookup k with
  | some v => Host.hostLog s!"{k} => {v}"; return v
  | none => throw (IO.userError s!"unknown key: {k}")
```

## Implement it in Rust

Write a function whose parameters and result are the Rust forms of the Lean types (see
[Lean types in Rust](../reference/type-mapping.md)). `IO α` becomes
`Result<A, lungo::IoError>`:

```rust
pub mod host {
    use lungo::{IoError, Nat};

    pub fn lookup(key: String) -> Option<Nat> {
        match key.as_str() {
            "alpha" => Some(Nat::from(1u64)),
            _ => None,
        }
    }

    pub fn log(line: String) -> Result<(), IoError> {
        if line.is_empty() {
            return Err(IoError::user("empty log line"));
        }
        eprintln!("{line}");
        Ok(())
    }
}
```

An `Err` returned from an `IO` function is raised in Lean as that `IO.Error`.

## Map the symbols

In `build.rs`, map each symbol to the Rust path of its implementation, as seen from the crate
root:

```rust
fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        .rust_extern("host_lookup", "crate::host::lookup")
        .rust_extern("host_log", "crate::host::log")
        .compile_lean("lean")
}
```

Build. If a Lean declaration's symbol has no implementation, the build fails with
[`LNG0401`](../reference/errors.md#lng0401), naming the declaration and the Rust signature it
needs. A mapping for a symbol nothing uses fails with
[`LNG0405`](../reference/errors.md#lng0405).

## Pass functions across the boundary

A Lean function parameter arrives in Rust as a `LeanClosure`, which Rust can call:

```lean
@[extern "host_transform"]
opaque hostTransform (f : Nat → Nat) (xs : List Nat) : List Nat
```

```rust
use lungo::{LeanClosure, List, Nat};

pub fn transform(f: LeanClosure<fn(Nat) -> Nat>, xs: List<Nat>) -> List<Nat> {
    xs.into_iter().map(|x| f.call(x)).collect()
}
```

In the other direction, Rust passes its own functions to generated Lean functions with
`LeanClosure::from_fn(|n: Nat| …)`.

## In `LeanOracle` mode

The same mappings work in `LeanOracle` mode for externs declared `@[extern "symbol"]`: the
generated adapter is exported under that C symbol. Other extern forms fail with
[`LNG0407`](../reference/errors.md#lng0407).

A complete example is `compiler-tests/host` in the repository.
