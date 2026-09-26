---
title: "How to compare generated Rust with Lean's native backend"
description: "Build one Lean project in PureRust and LeanOracle modes in one crate, and check that both give the same results."
---

This guide shows how to build the same Lean project both as generated Rust (`PureRust`) and on
Lean's own native backend (`LeanOracle`) in one crate, and to check that both give the same
results. Use it to test the Rust translation of your own Lean code.

`LeanOracle` builds for the host only, and on Windows needs a `*-windows-gnu` Rust target
(see [platforms](../reference/platforms.md)).

## Generate both modes

Give each mode its own name in `build.rs`:

```rust
use lungo_build::Mode;

fn main() -> lungo_build::Result<()> {
    for (name, mode) in [("pure", Mode::PureRust), ("oracle", Mode::LeanOracle)] {
        lungo_build::configure().mode(mode).name(name).compile_lean("lean")?;
    }
    Ok(())
}
```

and include each as its own module:

```rust
pub mod pure {
    lungo::include_lean!("pure");
}

pub mod oracle {
    lungo::include_lean!("oracle");
}
```

`LeanOracle` compiles the project's C code with the toolchain's `leanc` and links Lean's
runtime; the build script prints the link directives.

## Compare results

Both modules have the same functions and structurally identical types. Types declared by the
Lean project are distinct Rust types in each module, so build inputs separately for each and
compare results through a common form, such as their `Debug` rendering:

```rust
use lungo::Nat;

#[test]
fn both_backends_agree() {
    for (a, b) in [(0u64, 0u64), (7, 3), (u64::MAX, 2)] {
        let p = pure::nat_ops(Nat::from(a), Nat::from(b));
        let o = oracle::nat_ops(Nat::from(a), Nat::from(b));
        assert_eq!(format!("{p:?}"), format!("{o:?}"));
    }
}
```

`Debug` output of floats is exact (it distinguishes `-0.0` and NaN), so it compares floating
point results faithfully.

For broad coverage, generate inputs with a property-testing library such as `proptest`.
`compiler-tests/facade-runner` in the repository compares every function of a Lean library
this way.
