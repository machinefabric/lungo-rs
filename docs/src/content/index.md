---
title: "lungo documentation"
description: "Tutorials, how-to guides, reference and explanation for generating Rust, C, Go, Python, Swift and TypeScript from Lean projects."
---

lungo generates code from Lean projects: a Rust module at Cargo build time, and with the
`lungo` command packages for C, Go, Python, Swift (and Objective-C) and TypeScript. Lean's
own frontend checks and compiles the project; lungo turns the compiled program into each
language with an idiomatic API, running on one runtime.

## Tutorials

Start here if you are new to lungo.

- [Your first Rust crate built from Lean](tutorials/first-crate.md)
- [Your first Go package built from Lean](tutorials/first-go-package.md)
- [Your first Python package built from Lean](tutorials/first-python-package.md)
- [Your first Swift package built from Lean](tutorials/first-swift-package.md)
- [Your first TypeScript package built from Lean](tutorials/first-typescript-package.md)
- [Your first C library built from Lean](tutorials/first-c-library.md)

## How-to guides

- [How to call Rust functions from Lean](how-to/call-rust-from-lean.md)
- [How to call host code from Lean](how-to/call-host-code-from-lean.md) (Go, Python, Swift, TypeScript, C)
- [How to distribute generated packages](how-to/distribute-generated-packages.md)
- [How to write a generator plugin](how-to/write-a-generator-plugin.md)
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
- [The `lungo` command](reference/lungo-cli.md)
- [Generated packages](reference/generated-packages.md) (C, Go, Python, Swift, TypeScript)
- [The C API (`lungo.h`)](reference/c-api.md)
- [Wire format](reference/wire-format.md)
- [Generator plugins](reference/plugins.md)
- [Generated code](reference/generated-code.md)
- [Lean types in Rust](reference/type-mapping.md)
- [Errors](reference/errors.md)
- [Supported platforms](reference/platforms.md)

The Rust API of the `lungo` and `lungo-build` crates is documented by rustdoc:
`cargo doc -p lungo -p lungo-build --open`.

## Explanation

- [Architecture](explanation/architecture.md)
- [The runtime distribution](explanation/distribution.md)
- [The compiler boundary](explanation/compiler-boundary.md)
- [The runtime](explanation/runtime.md)
- [Trust and verification](explanation/trust-and-verification.md)
