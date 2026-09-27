# lungo-build

Compiles a Lake project into Rust at Cargo build time. Lean's own frontend elaborates and
kernel-checks the project and Lean's compiler compiles it; lungo-build turns the compiled
program into Rust on the [`lungo`](https://crates.io/crates/lungo) runtime, with an idiomatic
Rust API.

```toml
[dependencies]
lungo = "0.54.2456"

[build-dependencies]
lungo-build = "0.54.2456"
```

Use the same version of both: generated code runs on exactly the runtime it was generated for.

```rust
// build.rs
fn main() -> lungo_build::Result<()> {
    lungo_build::compile_lean("lean")
}
```

```rust
// src/lib.rs
pub mod formal {
    lungo::include_lean!("formal");
}
```

`compile_lean` builds what `lake build` builds, the project's default targets, and names the
generated module after the Lake package. `lungo_build::configure()` changes that and shapes
the generated types:

```rust
fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        .type_attribute("Formal", "#[derive(serde::Serialize, serde::Deserialize)]")
        .rust_extern("host_log", "crate::host::log")
        .compile_lean("lean")
}
```

The Lake project needs the toolchain its `lean-toolchain` pins, installed with
[elan](https://github.com/leanprover/elan). See the
[documentation](https://lungo.machinefabric.com/docs).
