# How to share types between Lean projects

This guide shows how to use the Rust types generated for one Lake package in the Rust module
of another package that requires it, so that values pass between the two modules without
conversion.

Suppose the Lake package `drawing` requires the package `geometry`, and both are compiled into
one crate. Built separately, each generated module has its own `Point` for the Lean structure
`Geometry.Point`, and a `Point` from one cannot be passed to a function of the other.

## Compile both packages

Compile each package in `build.rs`, and tell the build of `drawing` which Rust type
provides each Lean type of `geometry` it uses:

```rust
fn main() -> patina_build::Result<()> {
    patina_build::compile_lean("lean/geometry")?;
    patina_build::configure()
        .extern_type("Geometry.Point", "crate::geometry::Point")
        .extern_type("Geometry.Shape", "crate::geometry::Shape")
        .compile_lean("lean/drawing")
}
```

The Rust paths are as seen from the crate root; each module is named after its Lake package:

```rust
pub mod geometry {
    patina::include_lean!("geometry");
}

pub mod drawing {
    patina::include_lean!("drawing");
}
```

## Use the shared types

`drawing`'s functions now take and return `geometry`'s types:

```rust
use my_crate::{drawing, geometry::Point};
use patina::Int;

let a = Point { x: Int::from(-4), y: Int::from(2) };
let b = Point { x: Int::from(8), y: Int::from(6) };
let mid: Point = drawing::midpoint(a, b);
```

`cargo patina mappings` for `drawing` lists `Geometry.Point` with kind `extern type`.

## What an extern type must be

The Rust type represents the Lean type's values as patina represents them: it implements
`patina::LeanType` for the backend in use and has the Lean type's parameters. A type generated
by another patina build of the same Lean type, in the same mode, is such a type.

Types of `drawing` that contain an extern type derive only `Clone` and `Debug`, since patina
does not know which other traits the extern type implements; add further derives with
`type_attribute`. A mapping for a Lean type that no exported declaration uses fails the build
with [`PTN0107`](../reference/errors.md#ptn0107).

`compiler-tests/shaping` in the repository builds two such packages.
