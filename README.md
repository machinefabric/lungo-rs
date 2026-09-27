# lungo

lungo turns an ordinary Lake project into Rust code generated at Cargo build time. Lean's
own frontend elaborates and kernel-checks the project, and Lean's compiler compiles it; lungo
reproduces the compiled program on a Rust port of Lean's runtime and wraps it in an idiomatic
Rust API. No Lean runtime or C code is linked.

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

The Lake project in `lean` (package `formal`) is built as Lake builds it, from its default
targets, and its Lean definitions are then ordinary Rust functions and types in `formal`.
`lungo_build::configure()` changes what is compiled and exported and shapes the generated
types, for example to derive `serde` traits.

## Documentation

The documentation is published at <https://jowharshamshiri.github.io/lungo/docs>.

- New to lungo: [Your first Rust crate built from Lean](https://jowharshamshiri.github.io/lungo/docs/tutorials/first-crate)
- Everything else: how-to guides, reference (configuration, command line, generated code,
  errors) and explanation (architecture, trust)

Its sources are the Markdown files in `docs/src/content`; `docs/` is the site that publishes
them (see [Documentation site](#documentation-site)).

## Requirements

- Rust 1.89 or later
- [elan](https://github.com/leanprover/elan) with the toolchain the Lake project pins;
  `leanprover/lean4:v4.34.1` is supported

## Repository

| Path | Contents |
| --- | --- |
| `crates/lungo-bir` | The Bridge IR data model and its verifier |
| `crates/lungo-protocol` | The versioned worker protocol |
| `crates/lungo-runtime` | The Rust port of Lean's runtime |
| `crates/lungo` | Runtime support and facade types used by generated code |
| `crates/lungo-codegen` | The Rust backend |
| `crates/lungo-build` | Build-script integration, and in `worker/` the Lean worker, which reads Lean's compiler output and emits Bridge IR |
| `crates/cargo-lungo` | The `cargo lungo` command |
| `examples/session` | A Lean state machine with proofs, used from Rust |
| `compiler-tests/` | Differential, property, integration and corpus tests |
| `runtime-tests/` | Inventories of the toolchain's native interface |
| `docs/` | The documentation site; its pages are in `docs/src/content` |

## Testing

```sh
elan toolchain install leanprover/lean4:v4.34.1
cargo test --workspace --all-features
cargo check --manifest-path compiler-tests/stdlib-corpus/Cargo.toml
```

## Documentation site

`docs/` is a SvelteKit site (mdsvex, Tailwind CSS), deployed to GitHub Pages by
`.github/workflows/docs.yml`. Pages are the Markdown files in `docs/src/content`, each with a
`title` and `description` in its front matter; links between them are relative links to the
Markdown files, which work on GitHub and on the site alike.

```sh
cd docs
npm ci
npm run dev      # http://localhost:5173
npm run build    # the static site, in docs/build
```

## Contributing

Issues and pull requests are welcome: bug reports, questions, documentation fixes, and code.
For a larger change, opening an issue first to talk it over is a good start. Everyone taking
part is expected to follow the [code of conduct](CODE_OF_CONDUCT.md): be kind and respectful.

Unless you say otherwise, a contribution you submit is licensed under the Apache License,
Version 2.0, like the rest of the project.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
