# patina

patina turns an ordinary Lake project into Rust code generated at Cargo build time. Lean's
own frontend elaborates and kernel-checks the project, and Lean's compiler compiles it; patina
reproduces the compiled program on a Rust port of Lean's runtime and wraps it in an idiomatic
Rust API. No Lean runtime or C code is linked.

```rust
// build.rs
fn main() -> patina_build::Result<()> {
    patina_build::Config::new("lean")
        .root_module("Formal.Session")
        .export_module("Formal")
        .compile()
}
```

```rust
// src/lib.rs
pub mod formal {
    include!(concat!(env!("OUT_DIR"), "/patina/formal.rs"));
}
```

Lean definitions in `lean/Formal/Session.lean` are then ordinary Rust functions and types in
`formal`.

## Documentation

- New to patina: [Your first Rust crate built from Lean](docs/tutorials/first-crate.md)
- Everything else: [docs/index.md](docs/index.md) — how-to guides, reference (configuration,
  command line, generated code, errors) and explanation (architecture, trust)

## Requirements

- Rust 1.89 or later
- [elan](https://github.com/leanprover/elan) with the toolchain the Lake project pins;
  `leanprover/lean4:v4.34.1` is supported

## Repository

| Path | Contents |
| --- | --- |
| `worker/` | The Lean worker: reads Lean's compiler output and emits Bridge IR |
| `crates/patina-bir` | The Bridge IR data model and its verifier |
| `crates/patina-protocol` | The versioned worker protocol |
| `crates/patina-runtime` | The Rust port of Lean's runtime |
| `crates/patina` | Runtime support and facade types used by generated code |
| `crates/patina-codegen` | The Rust backend |
| `crates/patina-build` | Build-script integration |
| `crates/cargo-patina` | The `cargo patina` command |
| `examples/session` | A Lean state machine with proofs, used from Rust |
| `compiler-tests/` | Differential, property, integration and corpus tests |
| `runtime-tests/` | Inventories of the toolchain's native interface |

## Testing

```sh
elan toolchain install leanprover/lean4:v4.34.1
cargo test --workspace
cargo check --manifest-path compiler-tests/stdlib-corpus/Cargo.toml
```
