---
title: "Supported platforms"
description: "The supported Lean toolchains, host and target platforms, and what each mode needs."
---

## Lean toolchains

| Toolchain | Commit | Status |
| --- | --- | --- |
| `leanprover/lean4:v4.34.1` | `5045d0056413266e57c625dcd7c365b10e377c52` | supported |

A project's `lean-toolchain` must name a supported toolchain exactly; any other value, including
floating names (`stable`, `nightly`), fails with [`LNG0102`](errors.md#lng0102) before
anything is compiled. The installed toolchain's `lean --version` must match the pin.

## Rust

Rust 1.89 or later, edition 2024.

## Hosts and modes

| Host | PureRust | LeanOracle | Worker memory limit |
| --- | --- | --- | --- |
| Linux | yes | yes | yes |
| macOS | yes | yes | no ([`LNG0104`](errors.md#lng0104)) |
| Windows, `*-windows-msvc` target | yes | no ([`LNG0104`](errors.md#lng0104)) | yes |
| Windows, `*-windows-gnu` target | yes | yes | yes |

- **PureRust** generates code for any Rust target, including cross-compilation targets; the
  generated code links no Lean runtime and no C code.
- **LeanOracle** links Lean's native runtime from the host toolchain and cannot
  cross-compile. On Windows it requires the GNU ABI, which Lean's runtime is built for.

## Requirements of the Lake project

- `lean-toolchain`, `lakefile.toml` or `lakefile.lean`, and `lake-manifest.json`.
- Dependencies locked in the manifest must be materialized (`lake update`, or
  `cargo lungo setup`). Builds never resolve, download or update dependencies.
- Root modules belong to the project's root package.
