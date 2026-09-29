---
title: "Lean types in Rust"
description: "How each Lean type appears in the generated Rust API, and the serde feature."
---

The facade represents each Lean type in an exported signature as follows. Types from the
`lungo` crate are documented in its API documentation (`cargo doc -p lungo --open`).

## Builtin types

| Lean | Rust |
| --- | --- |
| `Nat` | `lungo::Nat` (unbounded) |
| `Int` | `lungo::Int` (unbounded) |
| `Bool` | `bool` |
| `UInt8`, `UInt16`, `UInt32`, `UInt64`, `USize` | `u8`, `u16`, `u32`, `u64`, `usize` |
| `Int8`, `Int16`, `Int32`, `Int64`, `ISize` | `i8`, `i16`, `i32`, `i64`, `isize` |
| `Float`, `Float32` | `f64`, `f32` |
| `Char` | `char` |
| `String` | `String` |
| `Unit`, `PUnit` | `()` |
| `ByteArray` | `lungo::ByteArray` |
| `FloatArray` | `lungo::FloatArray` |
| `Option α` | `Option<A>` |
| `List α` | `lungo::List<A>` |
| `Array α` | `Vec<A>` |
| `α × β` | `(A, B)` |
| `Except ε α` | `Result<A, E>` |

## Effects

| Lean result type | Rust result type |
| --- | --- |
| `IO α` | `Result<A, lungo::IoError>` |
| `EIO ε α` | `Result<A, E>` |
| `BaseIO α` | `A` |

`IoError::message()` is Lean's rendering of the error (`IO.Error.toString`);
`IoError::user(msg)` builds `IO.userError msg`.

## Functions

A parameter or result of function type `α₁ → … → αₙ → β` is
`lungo::LeanClosure<fn(A1, …, An) -> B>`. `call(a1, …, an)` applies it;
`LeanClosure::from_fn(f)` wraps a Rust function (`Fn + Send + Sync + 'static`). Arities 1 to 8
are supported. Functions returning `IO` are opaque (see below).

## Polymorphism

Type parameters become generic parameters bounded by `lungo::LeanType`. Instance arguments
become ordinary parameters whose type is the class structure, when it is first-order.

## Inductive types

A first-order inductive type (whose fields all have a representation in this table) becomes a
Rust type with the same parameters:

| Lean | Rust |
| --- | --- |
| structure | `struct` with named fields |
| constructors without fields only | fieldless `enum` |
| several constructors | `enum`; variants with named fields when every argument is named, tuple variants otherwise |
| one constructor, not a structure | `struct`, named or tuple as above |

Recursive fields are boxed (`Box<T>`). Types derive `Clone` and `Debug`; `PartialEq` unless
a field is a function or opaque; `Eq` and `Hash` unless a field is also a float. A type the
application provides with `extern_type` stops these derives for the types containing it, and
further derives are added with `type_attribute` (see
[configuration](https://machinefabric.com/lungo/docs/reference/configuration#shaping-the-generated-code)).

## serde

With the `serde` feature of the `lungo` crate, the facade types implement `serde`'s
`Serialize` and `Deserialize`, so generated types can derive them through `type_attribute`:

```toml
[dependencies]
lungo = { version = "1.80.12", features = ["serde"] }
```

| Rust | Serialized as |
| --- | --- |
| `lungo::Nat`, `lungo::Int` | a decimal string (`"12"`, `"-3"`), since the values are unbounded; deserialized from such a string or from an integer |
| `lungo::List<A>` | a sequence |
| `lungo::ByteArray` | bytes |
| `lungo::FloatArray` | a sequence of floats |

Opaque values and closures have no serialized form.

## Opaque values

Values of any other type (dependent types, types with proofs or type-valued fields, functions
returning `IO`) are `lungo::LeanValue<M>`, where `M` is a marker type in `__opaque` named
after the type's head constant. They can be stored, cloned and passed back to Lean.

## Names

| Lean | Rust |
| --- | --- |
| namespace component | snake_case module |
| type, constructor | CamelCase |
| function, field, parameter | snake_case |
| Rust keyword | raw identifier (`r#type`) or `_` suffix where raw identifiers are not allowed |

`UInt`/`USize`/`ISize` are single words when converting case (`toUInt8` → `to_uint8`).
Clashing names receive numeric suffixes in Lean-name order. Every mapping is recorded in
[`names.json`](https://machinefabric.com/lungo/docs/reference/generated-code#namesjson).
