# lungo

The runtime of Rust code generated from Lean by
[`lungo-build`](https://crates.io/crates/lungo-build), and the Rust types its API uses for
Lean's builtin types: `Nat` and `Int` (unbounded), `List`, `ByteArray`, `FloatArray`, `IoError`,
and handles for Lean closures and values.

Generated modules are included with `include_lean!`:

```rust
pub mod formal {
    lungo::include_lean!("formal");
}
```

## Features

- `serde`: `Serialize` and `Deserialize` for the facade types, so that generated types can
  derive them (`lungo_build::Builder::type_attribute`). `Nat` and `Int` serialize as decimal
  strings.

See the [documentation](https://lungo.machinefabric.com/docs).
