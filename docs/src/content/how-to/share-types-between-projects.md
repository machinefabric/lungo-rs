---
title: "How to share types between Lean projects"
description: "Use the types generated for one Lake package in the package generated for a Lake package that requires it, in Rust and in every other language."
---

This guide shows how to use the types generated for one Lake package in the package generated
for another package that requires it, so that values pass between the two without conversion.

Suppose the Lake package `drawing` requires the package `geometry`. Generated separately, each
has its own type for the Lean structure `Geometry.Point`, and a `Point` from one cannot be
passed to a function of the other. Telling the generation of `drawing` which type provides
`Geometry.Point` (an *extern type*) makes it use `geometry`'s.

## In Rust

Compile each package in `build.rs`, and tell the build of `drawing` which Rust type provides
each Lean type of `geometry` it uses:

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

`drawing`'s functions now take and return `geometry`'s types:

```rust
use my_crate::{drawing, geometry::Point};
use lungo::Int;

let a = Point { x: Int::from(-4), y: Int::from(2) };
let b = Point { x: Int::from(8), y: Int::from(6) };
let mid: Point = drawing::midpoint(a, b);
```

A type whose values Lean code alone can make (one with a proof among its fields) is opaque:
`geometry`'s module refers to it as `lungo::LeanValue<crate::geometry::__opaque::<Name>>`,
and that is the path to give.

## In the other languages

Each language's table of `lungo.toml` names the other package's type for a Lean type
(see [extern types](../reference/configuration.md#extern-types)):

```toml
# lungo.toml of drawing
project = "lean/drawing"

[go]
out = "drawing"

[go.extern-types."Geometry.Point"]
package = "example.com/app/geometry"   # geometry's generated Go package
name = "Point"
```

`package` is what the language imports (a Go import path, a Python module, a Swift module, a
JavaScript module specifier, a C header) and `name` the type as `geometry`'s generator named
it. Values made by one program pass to the other as they are — by handle for opaque types,
since both run on the process's one runtime. In TypeScript every program runs in a
WebAssembly instance of its own, so only types whose values are plain data can be shared
there; an opaque type is refused.

## What makes it safe

`drawing`'s compiled code reads `geometry`'s values at the layout it was compiled for. Both
must come from the same definition of the type — and of every type its fields reach. Every
generated type carries a layout fingerprint, and `drawing` checks `geometry`'s before
anything runs: in Rust it does not compile against another (the provided type implements
`lungo::LeanLayout`, which a type lungo generates does already), and in the other languages
the package refuses to load, naming the type and the providing package. The fix is to
regenerate both from the same Lean definition.

Types of `drawing` that contain a Rust extern type derive only `Clone` and `Debug`, since
lungo does not know which other traits the extern type implements; add further derives with
`type_attribute`. A mapping for a Lean type that no exported declaration uses fails with
[`LNG0107`](../reference/errors.md#lng0107).

`compiler-tests/shaping` builds two such Rust modules, and `compiler-tests/extern` shares
types between two programs in every language.
