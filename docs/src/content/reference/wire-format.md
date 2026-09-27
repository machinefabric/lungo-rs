---
title: "Wire format"
description: "The binary encoding of Lean values between a program and the language bindings: type expressions, values, type tables, signatures, handles and host functions."
---

Every language binding but Rust calls a Lean program through its C entry points, passing
arguments and receiving results in one binary encoding. This page specifies it. The runtime
implements the one codec that converts it to and from Lean objects; each language's support
library implements the encoding of its own values, and is tested against the shared vectors
of `compiler-tests/wire/vectors.json` (below).

Everything is little-endian. A *length* or *count* is a `u32`. Every read is bounds-checked;
data that is truncated, has bytes left over, or holds an invalid value is rejected, never
misread.

## Type expressions

A type expression is a tag byte, followed by its parts:

| Tag | Type | Parts |
| --- | --- | --- |
| 0 | `Nat` | |
| 1 | `Int` | |
| 2 | `Bool` | |
| 3–7 | `UInt8`, `UInt16`, `UInt32`, `UInt64`, `USize` | |
| 8–12 | `Int8`, `Int16`, `Int32`, `Int64`, `ISize` | |
| 13, 14 | `Float`, `Float32` | |
| 15 | `Char` | |
| 16 | `String` | |
| 17 | `Unit` | |
| 18 | `ByteArray` | |
| 19 | `FloatArray` | |
| 20 | `Option α` | α |
| 21 | `List α` | α |
| 22 | `Array α` | α |
| 23 | `α × β` | α, β |
| 24 | `Except ε α` | ε, α |
| 25 | `α₁ → … → αₙ → β` | `u32` n (1 to 15), α₁ … αₙ, β |
| 26 | type parameter | `u32` index |
| 27 | inductive type | `u32` index in the type table, `u32` count, the arguments |
| 28 | opaque value | |

A type parameter refers to a parameter of the enclosing inductive type (in the type table)
or function (in a signature). Values are always encoded at types without parameters: a call
of a polymorphic function passes its type arguments first (see [calls](#calls)).

## Values

| Type | Encoding |
| --- | --- |
| `Nat` | the magnitude: a length and its bytes, least significant first, without a most significant zero byte (so zero is the empty magnitude). |
| `Int` | a sign byte (0 non-negative, 1 negative), then the magnitude. Negative zero is invalid. |
| `Bool` | one byte, 0 or 1. |
| `UInt8` … `UInt64`, `Int8` … `Int64` | the value in 1, 2, 4 or 8 bytes (two's complement for signed types). |
| `USize`, `ISize` | 8 bytes. A value the platform's `USize` cannot hold is rejected by the runtime. |
| `Float`, `Float32` | the IEEE 754 bits in 8 or 4 bytes; every bit pattern, NaN payloads included, is preserved. |
| `Char` | a `u32` Unicode scalar value (not a surrogate, at most 0x10FFFF). |
| `String` | a length and that many bytes of valid UTF-8 (which may contain NUL). |
| `Unit` | nothing. |
| `ByteArray` | a length and the bytes. |
| `FloatArray` | a count and that many 8-byte floats. |
| `Option α` | 0 (`none`), or 1 and a value of α (`some`). |
| `List α`, `Array α` | a count and the values. |
| `α × β` | a value of α, then of β. |
| `Except ε α` | 0 and a value of ε (`error`), or 1 and a value of α (`ok`). |
| function | a kind byte and a `u64`: 0 and a [handle](#handles) (a Lean closure), or 1 and a [host function](#host-functions)'s identifier. |
| opaque value | a `u64` [handle](#handles). |
| inductive type | a `u32` constructor index (in declaration order) and the constructor's fields in order, each at its type with the type's arguments substituted; a structure the table marks *trivial* (one constructor whose runtime representation is one field) is encoded as that field alone. |

## Type tables

A program describes its inductive types in a *type table*, which the generated code embeds
and every generator receives (as JSON in a [plugin request](plugins.md)). In its binary
form it is the magic `LNGT`, a `u32` version (1), a count, and per type: its name (a length
and UTF-8), its number of parameters, its runtime representation, whether it is trivial
(and which constructor and field), and its constructors: name, runtime tag, object, `usize`
and scalar sizes, and fields (name, where the field is stored, and its type expression).
The runtime validates a table before using it; generated tables are valid by construction.

## Signatures

The signature of a function crossing the boundary is its number of type parameters, a count
and the parameters' type expressions, and what it returns: 0 and a type (a value), 1 and a
type (`IO α`), or 2 and two types (`EIO ε α`: the error, then the value).

## Calls

A generated entry point `int32_t <prefix>call_<function>(const uint8_t *input, size_t len,
lungo_buffer *out)` takes as input: a count of type arguments and their type expressions
(one per type parameter, without parameters themselves), then the arguments. It returns 0
and stores the result in `out`, or 1 and stores a UTF-8 message saying why the input was
rejected. The result is:

- for a value: the value;
- for `IO α`: 0 and the value, or 1 and an `IO.Error`: a handle and its message (a length and
  UTF-8, as `IO.Error.toString` renders it);
- for `EIO ε α`: 0 and the value, or 1 and the error value.

`BaseIO α` functions return their value directly. A closure is called with
`lungo_closure_call` the same way (its type arguments are always none).

## Handles

A handle (a nonzero `u64`) names a Lean object the runtime holds for the host: an opaque
value, a Lean closure, an `IO.Error`. Handles in data the runtime *produces* are new, and
owned by the receiver, which releases each with `lungo_handle_release` once (a support
library does so when the value is collected or closed). In data a host *sends*:

- the arguments of a call *lend* their handles: the host keeps them;
- the result of a host function *gives* its handles to the runtime, which releases them: a
  host returning a value it received passes a clone (`lungo_handle_clone`).

An `IO.Error` a host function raises is a handle and a message: a handle it received from
Lean (a clone), or 0 and the message, which becomes `IO.userError message`.

## Host functions

The host (the one support library of the process) installs three entry points with
`lungo_set_host` (on WebAssembly they are the module's imports `lungo.dispatch`,
`lungo.retain` and `lungo.release`):

- `dispatch(callback, input, len, out)` runs host function `callback` on its encoded
  arguments and stores its encoded result in `out` (allocated with `lungo_buffer_alloc`),
  returning 0, or stores a UTF-8 message and returns nonzero if it failed;
- `retain(callback)` and `release(callback)` count the runtime's references to it.

A host function in the arguments of a call is retained by the runtime for as long as a Lean
closure holds it (the host keeps it alive during the call); one in a host function's result
comes with a reference the host gives up. The result of a host function implementing an
`IO` extern is 0 and the value, or 1 and an `IO.Error`; of an `EIO ε` extern 0 and the value,
or 1 and the error. A failure of a host function where Lean cannot observe it (a pure
function) terminates the program with its message.

## Test vectors

`compiler-tests/wire/vectors.json` holds valid encodings (type expression, value, bytes)
and invalid ones (type expression, bytes) that every codec must reject. Values are JSON:
numbers that may exceed 53 bits (`Nat`, `Int`, 64-bit integers) are decimal strings; floats
are `{"bits": "<hex>"}`; `Unit` is `null`; byte arrays `{"bytes": "<hex>"}`; options
`{"none": null}` or `{"some": v}`; `Except` `{"error": e}` or `{"ok": v}`; pairs two-element
arrays; functions `{"lean": "<handle>"}` or `{"host": "<id>"}`; opaque values
`{"handle": "<handle>"}`. The runtime's tests regenerate the file (`LUNGO_BLESS=1`) and check
it against the runtime's codec; the support libraries of Go, Python, Swift and TypeScript
check themselves against it.
