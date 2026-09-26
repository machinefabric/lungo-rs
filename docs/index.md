# lungo documentation

lungo generates Rust from Lean projects at Cargo build time. Lean's own frontend checks and
compiles the project; lungo turns the compiled program into Rust with an idiomatic API.

## Tutorials

Start here if you are new to lungo.

- [Your first Rust crate built from Lean](tutorials/first-crate.md)

## How-to guides

- [How to call Rust functions from Lean](how-to/call-rust-from-lean.md)
- [How to run a Lean program's `main` from Rust](how-to/run-a-lean-program.md)
- [How to derive `serde` and other traits for generated types](how-to/shape-generated-types.md)
- [How to share types between Lean projects](how-to/share-types-between-projects.md)
- [How to see what lungo generated for a declaration](how-to/inspect-generated-code.md)
- [How to reject exports that rely on `sorry`, axioms or unsafe code](how-to/enforce-trust-policies.md)
- [How to compare generated Rust with Lean's native backend](how-to/compare-with-native-lean.md)
- [How to bound the resources a build may use](how-to/bound-the-worker.md)
- [How to diagnose a failing lungo build](how-to/diagnose-build-failures.md)

## Reference

- [Configuration](reference/configuration.md)
- [`cargo lungo`](reference/cli.md)
- [Generated code](reference/generated-code.md)
- [Lean types in Rust](reference/type-mapping.md)
- [Errors](reference/errors.md)
- [Supported platforms](reference/platforms.md)

The Rust API of the `lungo` and `lungo-build` crates is documented by rustdoc:
`cargo doc -p lungo -p lungo-build --open`.

## Explanation

- [Architecture](explanation/architecture.md)
- [The compiler boundary](explanation/compiler-boundary.md)
- [The runtime](explanation/runtime.md)
- [Trust and verification](explanation/trust-and-verification.md)
