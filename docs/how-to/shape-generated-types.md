# How to derive `serde` and other traits for generated types

This guide shows how to add attributes to the Rust types patina generates, such as
`serde` derives, and how to control their `Debug` implementations and documentation.

## Derive `serde`

Enable the `serde` feature of `patina`, which implements `serde` for `Nat`, `Int`, `List` and
the other facade types, and depend on `serde` yourself:

```toml
[dependencies]
patina = { path = "../patina/crates/patina", features = ["serde"] }
serde = { version = "1", features = ["derive"] }
```

Add the derive to the types of a namespace in `build.rs`:

```rust
fn main() -> patina_build::Result<()> {
    patina_build::configure()
        .type_attribute("Geometry", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_lean("lean")
}
```

The path `Geometry` selects every type in the Lean namespace `Geometry`; `.` selects every
type, and `Geometry.Point` only that one. `Nat` and `Int` values serialize as decimal strings
(`{"x": "3", "y": "-4"}`) because they are unbounded.

## Add attributes to structs, enums and fields

`struct_attribute` and `enum_attribute` select only structs or only enums, and
`field_attribute` selects fields by their Lean names: `<Structure>.<field>` for a structure,
`<Constructor>.<binder>` for other constructors.

```rust
patina_build::configure()
    .type_attribute("Geometry", "#[derive(serde::Serialize, serde::Deserialize)]")
    .struct_attribute("Geometry.Point", "#[serde(deny_unknown_fields)]")
    .enum_attribute("Geometry.Shape", r#"#[serde(tag = "kind", rename_all = "snake_case")]"#)
    .field_attribute("Geometry.Point.x", r#"#[serde(rename = "col")]"#)
    .field_attribute("Geometry.Shape.segment.start", r#"#[serde(rename = "from")]"#)
    .compile_lean("lean")
```

`cargo patina mappings` lists the Lean name of every generated type and field. A path that
selects nothing fails the build with [`PTN0107`](../reference/errors.md#ptn0107), so a
misspelled or stale path does not go unnoticed.

## Implement `Debug` yourself

Generated types derive `Debug`. To format a type yourself, for example to keep a secret out of
logs, skip the derive:

```rust
patina_build::configure().skip_debug(["Geometry.Secret"]).compile_lean("lean")
```

and implement it next to the generated module:

```rust
pub mod geometry {
    patina::include_lean!("geometry");

    impl std::fmt::Debug for Secret {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Secret(<redacted>)")
        }
    }
}
```

## Omit documentation comments

Generated types and functions carry doc comments with their Lean name, type and source.
`disable_comments` omits them for the types and functions a path selects:

```rust
patina_build::configure().disable_comments(["Geometry.Internal"]).compile_lean("lean")
```

## In `patina.toml`

The same settings in `patina.toml`, for `cargo patina`:

```toml
project = "lean"

[build]
type-attributes = [
  { path = "Geometry", attribute = "#[derive(serde::Serialize, serde::Deserialize)]" },
]
field-attributes = [{ path = "Geometry.Point.x", attribute = '#[serde(rename = "col")]' }]
skip-debug = ["Geometry.Secret"]
```

The settings are listed in the [configuration reference](../reference/configuration.md#shaping-the-generated-code).
`compiler-tests/shaping` in the repository uses each of them.
