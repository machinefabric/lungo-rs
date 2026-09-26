# How to see what lungo generated for a declaration

This guide shows how to find out what a Lean declaration became: its Rust name and signature,
the Lean compiler output it came from, and what it depends on.

## Set up the command line

Install `cargo lungo` from the repository:

```sh
cargo install --path crates/cargo-lungo
```

It reads the build configuration from `lungo.toml` in the package directory. Write one
that matches your `build.rs`: the Lake project, and in `[build]` the settings `build.rs`
changes (see [configuration](../reference/configuration.md)). For
`configure().export_module("Formal").compile_lean("lean")`:

```toml
project = "lean"

[build]
export-modules = ["Formal"]
```

Without the file, pass the Lake project and settings as options:
`--project lean --export-module Formal`. A `build.rs` that only calls
`compile_lean("lean")` needs only `--project lean`.

## Find the Rust name of a Lean name

```sh
cargo lungo mappings
```

prints every mapping (items, constructors and fields), for example:

```json
  {
    "lean_name": "Formal.Sess.isOpen",
    "kind": "field",
    "rust_path": "Sess.is_open",
    "renamed": true
  },
```

The same file is generated as `names.json` in the build output.

## Inspect one declaration

```sh
cargo lungo inspect Formal.apply
```

shows its Lean type, module, source file, trust metadata, compiled signature and the
compiler auxiliaries Lean derived from it.

To see the code:

```sh
cargo lungo ir Formal.apply     # Lean's compiled form (Bridge IR)
cargo lungo rust Formal.apply   # the generated Rust
```

## Find out how externs are implemented

```sh
cargo lungo externs
```

lists every extern the program reaches, with whether the lungo runtime, a Lean
`@[export]` definition, or one of your `rust_extern` functions implements it.

## Read the metadata at run time

The generated module carries the same information for exported declarations:

```rust
let info = formal::__meta::declaration("Formal.apply").unwrap();
println!("{} is {} at {:?}", info.lean_name, info.rust_path, info.source_file);
```

See [generated code](../reference/generated-code.md) for every field and file.
