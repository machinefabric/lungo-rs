# lean2rust

lean2rust turns an ordinary Lake project into Rust code generated at Cargo build time. Lean's
own frontend elaborates and kernel-checks the project, and Lean's compiler compiles it; lean2rust
reproduces the compiled program on a Rust port of Lean's runtime and wraps it in an idiomatic
Rust API. No Lean runtime or C code is linked.

```rust
// build.rs
fn main() -> lean2rust_build::Result<()> {
    lean2rust_build::Config::new("lean")
        .root_module("Formal.Session")
        .export_module("Formal")
        .compile()
}
```

```rust
// src/lib.rs
pub mod formal {
    include!(concat!(env!("OUT_DIR"), "/lean2rust/formal.rs"));
}
```

Lean definitions in `lean/Formal/Session.lean` are then ordinary Rust functions and types in
`formal`.

## Documentation

- New to lean2rust: [Your first Rust crate built from Lean](docs/tutorials/first-crate.md)
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
| `crates/lean2rust-bir` | The Bridge IR data model and its verifier |
| `crates/lean2rust-protocol` | The versioned worker protocol |
| `crates/lean2rust-runtime` | The Rust port of Lean's runtime |
| `crates/lean2rust` | Runtime support and facade types used by generated code |
| `crates/lean2rust-codegen` | The Rust backend |
| `crates/lean2rust-build` | Build-script integration |
| `crates/cargo-lean2rust` | The `cargo lean2rust` command |
| `examples/session` | A Lean state machine with proofs, used from Rust |
| `compiler-tests/` | Differential, property, integration and corpus tests |
| `runtime-tests/` | Inventories of the toolchain's native interface |

## Testing

```sh
elan toolchain install leanprover/lean4:v4.34.1
cargo test --workspace
cargo check --manifest-path compiler-tests/stdlib-corpus/Cargo.toml
```
