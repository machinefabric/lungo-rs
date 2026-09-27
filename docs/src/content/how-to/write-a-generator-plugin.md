---
title: "How to write a generator plugin"
description: "Generate a package in a language lungo does not build in, with a lungo-gen-<language> program."
---

This guide shows how to add a language to `lungo generate` with a plugin, the way
`protoc-gen-<language>` programs add languages to `protoc`.

## Write the program

A plugin reads a `GenerateRequest` (JSON) on standard input and writes a `GenerateResponse`
(JSON) on standard output. The [protocol reference](../reference/plugins.md) lists every
field. In Rust, the `lungo-build` crate provides the types:

```rust
use lungo_build::codegen::plugin::{GenerateRequest, GenerateResponse};
use std::io::Read;

fn main() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).unwrap();
    let request: GenerateRequest = serde_json::from_str(&input).unwrap();
    let mut response = GenerateResponse::default();
    for key in request.options.keys() {
        response.errors.push(format!("unknown option `{key}`"));
    }
    if response.errors.is_empty() {
        let mut api = String::new();
        for f in &request.boundary.functions {
            api.push_str(&format!("// {} : {}\n// entry point: {}\n", f.lean_name, f.lean_type, f.symbol));
        }
        response.files.insert("api.txt".into(), api);
        // The program's C, which the package compiles and links against the runtime.
        response.files.extend(request.program_files.clone());
    }
    println!("{}", serde_json::to_string(&response).unwrap());
}
```

A generator for a real language:

1. copies `program_files`: the package compiles every `.c` file with `program/` on its
   include path, and links the runtime of `request.runtime` (by release URL and SHA-256, or
   from the local distribution);
2. emits the language's types for `boundary.table` and `boundary.types`, and a function per
   entry of `boundary.functions` that encodes its arguments in the
   [wire format](../reference/wire-format.md), calls the entry point `symbol`, and decodes
   the result per `returns`;
3. installs itself as the process's host (`lungo_set_host`) to receive calls of host
   functions, and registers the implementations of `boundary.host_externs` with
   `set_host_extern`.

The support libraries of the built-in languages (`runtimes/` in the repository) are complete
examples of the runtime side, and `compiler-tests/wire/vectors.json` tests a codec.

## Install and run it

Put the program on `PATH` as `lungo-gen-<language>` (`.exe` on Windows):

```sh
cargo install --path my-plugin   # installs lungo-gen-kotlin
lungo generate --kotlin_out=gen/kotlin --kotlin_opt=package=org.example
```

or configure it in `lungo.toml`:

```toml
[plugins.kotlin]
out = "gen/kotlin"
options = { package = "org.example" }
```

The output directory is replaced by the response's files. A plugin that fails, writes a
malformed response or reports errors fails the command with
[`LNG0504`](../reference/errors.md#lng0504), showing its standard error or its errors;
nothing is written.
