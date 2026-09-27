---
title: "Your first Rust crate built from Lean"
description: "Write Lean definitions and a proof, build them into a Rust crate, and call them from Rust."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, and call them from Rust as ordinary Rust functions. Along the way we will meet the
Lake project that holds the Lean code, the `build.rs` that turns it into Rust, and the
generated module that the Rust code uses.

It takes about fifteen minutes, most of which is the first build.

## Before we start

We need three things on the machine:

- Rust 1.89 or later (`cargo --version`)
- [elan](https://github.com/leanprover/elan), Lean's toolchain manager (`elan --version`)
- git

Install the Lean toolchain we will use:

```sh
elan toolchain install leanprover/lean4:v4.34.1
```

lungo is used from its repository. Make a working directory and clone it there:

```sh
mkdir lungo-tutorial
cd lungo-tutorial
git clone https://github.com/machinefabric/lungo.git
```

Everything else in this tutorial happens inside `lungo-tutorial`.

## Create the crate

Create a library crate called `shapes`:

```sh
cargo new --lib shapes
cd shapes
```

## Write the Lean code

Our Lean code lives in its own Lake project, in a directory `lean` inside the crate:

```sh
mkdir lean
cd lean
```

A Lake project needs three files. First, the toolchain it uses. Create `lean-toolchain`:

```text
leanprover/lean4:v4.34.1
```

Next, the Lake configuration. Create `lakefile.toml`:

```toml
name = "shapes"
version = "0.1.0"
defaultTargets = ["Shapes"]

[[lean_lib]]
name = "Shapes"
```

Now the Lean code itself. Create `Shapes.lean`:

```lean
namespace Shapes

structure Rect where
  width : Nat
  height : Nat

def area (r : Rect) : Nat := r.width * r.height

def scale (k : Nat) (r : Rect) : Rect :=
  { width := k * r.width, height := k * r.height }

theorem area_scale (k : Nat) (r : Rect) : area (scale k r) = k * k * area r := by
  simp only [area, scale]
  rw [Nat.mul_mul_mul_comm]

end Shapes
```

The theorem says that scaling a rectangle by `k` multiplies its area by `k * k`.

The third file is the manifest that records the project's dependencies. Let Lake create it:

```sh
lake update
```

Lake answers:

```text
info: shapes: no previous manifest, creating one from scratch
```

and a `lake-manifest.json` appears next to the other files. Our project has no dependencies,
but lungo always builds from a committed manifest (see
[Architecture](../explanation/architecture.md)).

Go back to the crate:

```sh
cd ..
```

## Connect Cargo to the Lean project

Open `Cargo.toml` and add the two lungo crates below the existing `[dependencies]` line:

```toml
[dependencies]
lungo = { path = "../lungo/crates/lungo" }

[build-dependencies]
lungo-build = { path = "../lungo/crates/lungo-build" }
```

`lungo` is what the generated code uses at run time; `lungo-build` runs at build time.

Create `build.rs` in the crate directory, next to `Cargo.toml`:

```rust
fn main() -> lungo_build::Result<()> {
    lungo_build::compile_lean("lean")
}
```

This tells lungo where the Lake project is (`lean`). Everything else comes from the
project itself: lungo compiles what `lake build` builds, the `Shapes` library named in
`defaultTargets`, and gives the declarations of `Shapes` public Rust functions.

Finally, replace the contents of `src/lib.rs` with:

```rust
pub mod shapes {
    lungo::include_lean!("shapes");
}
```

The generated code is not in `src`; it is written into Cargo's build directory, under the
name of the Lake package (`name = "shapes"` in `lakefile.toml`), and this line includes it
as the module `shapes`.

## Build

```sh
cargo build
```

The first build takes a few minutes: lungo compiles its Lean worker for the toolchain
once and keeps it for later builds. When it finishes, Cargo reports:

```text
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1m 45s
```

Lean has checked the proof of `area_scale`, compiled the definitions, and lungo has
generated Rust from them.

## Call Lean from Rust

Create `tests/rect.rs`:

```rust
use lungo::Nat;
use shapes::shapes::{Rect, area, scale};

#[test]
fn area_of_a_rectangle() {
    let r = Rect { width: Nat::from(3u64), height: Nat::from(4u64) };
    assert_eq!(area(r), Nat::from(12u64));
}

#[test]
fn scaling_multiplies_the_area() {
    let r = Rect { width: Nat::from(3u64), height: Nat::from(4u64) };
    let big = scale(Nat::from(10u64), r);
    assert_eq!(big.width, Nat::from(30u64));
    assert_eq!(area(big), Nat::from(1200u64));
}
```

Notice that the Lean structure `Rect` is a Rust struct with the same fields, and that
`area` and `scale` are plain functions. Lean's `Nat` is unbounded, so it is the Rust type
`lungo::Nat` rather than a fixed-width integer.

Run the tests:

```sh
cargo test
```

Among Cargo's output:

```text
test area_of_a_rectangle ... ok
test scaling_multiplies_the_area ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out
```

Both answers came from the compiled Lean definitions.

## Change the Lean code

Add a definition at the end of `lean/Shapes.lean`, just before `end Shapes`:

```lean
def perimeter (r : Rect) : Nat := 2 * (r.width + r.height)
```

Build again:

```sh
cargo build
```

This time it takes seconds. Cargo noticed that a Lean file changed and regenerated the
Rust; `shapes::perimeter` now exists. Add a test for it to `tests/rect.rs` if you like, and
run `cargo test` again.

## Break a proof

Let's see what happens when Lean disagrees. In `lean/Shapes.lean`, change the statement of
`area_scale` to something false:

```lean
theorem area_scale (k : Nat) (r : Rect) : area (scale k r) = k * area r := by
```

and build:

```sh
cargo build
```

The build fails, and Cargo shows Lean's own report:

```text
  Error: error[LNG0201]: Lean elaboration failed
  ...
  error: Shapes.lean:12:75: unsolved goals
  k : Nat
  r : Rect
  ⊢ k * k * (r.width * r.height) = k * (r.width * r.height)
```

No Rust was generated from the broken project: the crate cannot be built while its Lean
code does not check. `LNG0201` is the code of this kind of error; every lungo error has
one (see the [error reference](../reference/errors.md)).

Put the statement back to `k * k * area r` and run `cargo test`; everything passes again.

## What we have done

We built a Rust crate whose logic is written and proved in Lean: a Lake project inside the
crate, a three-line `build.rs`, and one `include_lean!`. The Rust code used Lean's structure
and functions directly, rebuilt itself when the Lean code changed, and refused to build when a
proof failed.

From here:

- [How to call Rust functions from Lean](../how-to/call-rust-from-lean.md)
- [Configuration reference](../reference/configuration.md)
- [How Lean types appear in Rust](../reference/type-mapping.md)
- [Architecture](../explanation/architecture.md), for what happened during the build
