---
title: "Supported platforms"
description: "The supported Lean toolchains, host and target platforms, the runtime's platforms, and what each language binding needs."
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

## The prebuilt runtime

Each release publishes the runtime (`lungo runtime fetch --target <triple>`) for:

| Target | Static | Shared | Used by |
| --- | --- | --- | --- |
| `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu` (glibc 2.28 or later) | yes | yes | C, Go, Python |
| `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl` | yes | yes | C |
| `x86_64-apple-darwin`, `aarch64-apple-darwin` (macOS 12 or later) | yes | yes | C, Go, Python, Swift |
| `aarch64-apple-ios`, `aarch64-apple-ios-sim`, `x86_64-apple-ios` (iOS 15 or later) | yes | | C, Swift |
| `x86_64-pc-windows-msvc` | yes | yes | C, Python |
| `x86_64-pc-windows-gnu` | yes | yes | C, Go (cgo builds with MinGW-w64) |
| `wasm32-wasip1` | yes | | TypeScript |

The Apple targets are also combined into one XCFramework (macOS universal, iOS, iOS
simulator universal), which the Swift package uses.

## Language bindings

| Language | Needs | Platforms |
| --- | --- | --- |
| C | CMake 3.20, a C11 compiler | every target above |
| Go | Go 1.22, cgo with a C compiler (MinGW-w64 GCC on Windows) | Linux (amd64, arm64), macOS (amd64, arm64), Windows (amd64) |
| Python | Python 3.9, CMake, a C compiler to install from source | Linux (x86_64, aarch64, glibc), macOS (x86_64, arm64), Windows (x86_64) |
| Swift, Objective-C | Swift 5.9 | macOS, iOS, iOS simulator |
| TypeScript | Node.js 20, or a browser | anywhere WebAssembly runs; no threads, child processes, sockets, signals or timers ([`LNG0408`](errors.md#lng0408)) |

`lungo` itself runs on Linux (x86_64, aarch64), macOS (x86_64, arm64) and Windows (x86_64).
The released command is statically linked on Linux (musl) and built for the GNU ABI on
Windows, which is also the target its `runtime` commands default to.

## Requirements of the Lake project

- `lean-toolchain`, `lakefile.toml` or `lakefile.lean`, and `lake-manifest.json`.
- Dependencies locked in the manifest must be materialized (`lake update`, or
  `lungo setup`). Builds never resolve, download or update dependencies.
- Root modules belong to the project's root package.
