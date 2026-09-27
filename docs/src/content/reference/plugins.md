---
title: "Generator plugins"
description: "The protocol between the lungo command and generator plugins: the GenerateRequest and GenerateResponse JSON documents."
---

A generator for a language lungo does not build in is a *plugin*: a program named
`lungo-gen-<language>` on `PATH` (`lungo-gen-<language>.exe` on Windows), run by
`lungo generate --<language>_out=DIR`, as `protoc` runs `protoc-gen-<language>`.

The plugin reads one `GenerateRequest` as JSON on its standard input and writes one
`GenerateResponse` as JSON on its standard output; its standard error is shown if it fails.
Built-in generators receive exactly the same request. The Rust types of both documents are
`lungo_build::codegen::plugin::{GenerateRequest, GenerateResponse}` (serde), which a plugin
in Rust can use directly. See [how to write a generator plugin](../how-to/write-a-generator-plugin.md).

## `GenerateRequest`

| Field | Meaning |
| --- | --- |
| `protocol_version` | `2`. A plugin rejects versions it does not know. |
| `program` | `name` (the program's name), `lean_version`, `lean_githash`, `bir_version`, `root_modules`. |
| `boundary` | The program's interface (below). |
| `program_files` | The program as C: paths (under `program/`) to contents, including `lungo.h`. A package compiles every `.c` file with `program/` on its include path. |
| `runtime` | `version` (the lungo release), `abi_version` (the C ABI), and `distribution`: `{"release": {"artifacts": {…}}}` (runtime archives by target triple and `xcframework`, each `{url, sha256}`), or `{"local": {"dir": …}}` (a [local distribution](lungo-cli.md#the-runtime): an absolute path with `/` separators, also on Windows). |
| `options` | The generator's options (`--<language>_opt`, `options` of `[plugins.<language>]`), strings by key. A plugin rejects keys it does not know. |
| `extern_types` | The program's types another generated package provides (`extern-types` of `[plugins.<language>]`): by Lean name, `{package, name}` in the language's terms. A plugin rejects one the boundary has no type for, uses the other package's type and descriptor instead of generating one, and checks the other package's [layout fingerprint](configuration.md#extern-types) before the program runs. |

### `boundary`

| Field | Meaning |
| --- | --- |
| `id` | The program's C identifier. |
| `prefix` | The prefix of the program's C symbols, `<id>__`. |
| `table` | The [type table](wire-format.md#type-tables): `types`, each with `name`, `params` (a count), `repr`, `trivial` (`[constructor, field]` or `null`), `ctors` (`name`, `tag`, `size`, `usize`, `ssize`, `fields`: `name`, `kind`, `ty`). |
| `types` | Per type of the table: `lean_name`, `params` (the Lean names of its parameters), `structure`, `opaque` (its values are handles: the table entry has no constructors), and `fingerprint` (its layout fingerprint, lowercase hexadecimal SHA-256), which the package exposes for packages using the type. |
| `functions` | Per exported function: `lean_name`, `module`, `lean_type`, `symbol` (its C entry point), `type_params`, `params` (`name`, `ty`), `returns`, `source`, `trust`. |
| `host_externs` | Per extern the host implements: `index` (for `set_host_extern`), `key`, `declaration`, `lean_type`, `params`, `returns`. Type parameters of polymorphic externs are opaque values. |
| `run_main` | The symbol of `int32_t run_main(size_t argc, const char *const *argv)`, or `null` without a `main`. |
| `set_host_extern` | `void set_host_extern(size_t index, uint64_t callback)`: registers a host function (see [host functions](wire-format.md#host-functions)). |
| `types_symbol` | `const lungo_types *types(void)`: the loaded type table. |
| `initialize` | `void initialize(void)`: every entry point initializes the program on first use; it requires every host extern to be registered. |

Types (`ty`, `returns`) are type expressions as JSON: `"nat"`, `"int"`, `"bool"`,
`"uint8"`, `"uint16"`, `"uint32"`, `"uint64"`, `"usize"`, `"int8"`, `"int16"`, `"int32"`,
`"int64"`, `"isize"`, `"float"`, `"float32"`, `"char"`, `"string"`, `"unit"`,
`"byte_array"`, `"float_array"` for the types without parts, `{"option": t}`, `{"list": t}`, `{"array": t}`, `{"prod": [a, b]}`,
`{"except": {"error": e, "value": v}}`, `{"function": {"params": […], "result": r}}`,
`{"param": i}`, `{"inductive": {"index": i, "args": […]}}`, `"opaque"`; `returns` is
`{"value": t}`, `{"io": t}` or `{"eio": {"error": e, "value": v}}`. Their meaning, and the
encoding of values at them, is the [wire format](wire-format.md).

## `GenerateResponse`

| Field | Meaning |
| --- | --- |
| `files` | The package's files: `/`-separated paths relative to the output directory, to contents (text). Paths must not be absolute or contain `..`. |
| `errors` | Why generation failed. When not empty, `files` is ignored. |

The output directory is replaced by exactly the response's files (and lungo's
`build-info.json`). A plugin that cannot be run, exits unsuccessfully, writes something that
is not a `GenerateResponse`, reports errors, or generates no files fails the command with
[`LNG0504`](errors.md#lng0504); nothing is published.
