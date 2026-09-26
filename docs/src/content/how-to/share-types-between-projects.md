---
title: "How to share types between Lean projects"
description: "Use the Rust types generated for one Lake package in the module of a package that requires it."
---

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
fn main() -> lungo_build::Result<()> {
    lungo_build::compile_lean("lean/geometry")?;
    lungo_build::configure()
        .extern_type("Geometry.Point", "crate::geometry::Point")
        .extern_type("Geometry.Shape", "crate::geometry::Shape")
        .compile_lean("lean/drawing")
}
```

The Rust paths are as seen from the crate root; each module is named after its Lake package:

```rust
pub mod geometry {
    lungo::include_lean!("geometry");
}

pub mod drawing {
    lungo::include_lean!("drawing");
}
```

## Use the shared types

`drawing`'s functions now take and return `geometry`'s types:

```rust
use my_crate::{drawing, geometry::Point};
use lungo::Int;

let a = Point { x: Int::from(-4), y: Int::from(2) };
let b = Point { x: Int::from(8), y: Int::from(6) };
let mid: Point = drawing::midpoint(a, b);
```

`cargo lungo mappings` for `drawing` lists `Geometry.Point` with kind `extern type`.

## What an extern type must be

The Rust type represents the Lean type's values as lungo represents them: it implements
`lungo::LeanType` for the backend in use and has the Lean type's parameters. A type generated
by another lungo build of the same Lean type, in the same mode, is such a type.

Types of `drawing` that contain an extern type derive only `Clone` and `Debug`, since lungo
does not know which other traits the extern type implements; add further derives with
`type_attribute`. A mapping for a Lean type that no exported declaration uses fails the build
with [`LNG0107`](../reference/errors.md#lng0107).

`compiler-tests/shaping` in the repository builds two such packages.
