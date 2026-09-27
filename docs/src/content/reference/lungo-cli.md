---
title: "The lungo command"
description: "lungo generate and the other commands: generating packages in every language, inspecting the program, and the runtime."
---

`lungo` generates code from a Lean program the way `protoc` does from `.proto` files: one
command, one output directory per language, and plugins for further languages.

```text
lungo [OPTIONS] <COMMAND>
lungo generate --c_out=gen/c --go_out=gen/go --python_out=gen/py --rust_out=gen/rust
```

## Install

**Homebrew (macOS and Linux):**

```sh
brew install machinefabric/tap/lungo
```

**apt (Debian, Ubuntu):**

```sh
curl -fsSL https://release.machinefabric.com/lungo-cli/keys/repo-signing-prod.asc \
  | sudo gpg --dearmor -o /usr/share/keyrings/lungo.gpg
echo "deb [signed-by=/usr/share/keyrings/lungo.gpg] https://release.machinefabric.com/lungo-cli/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/lungo.list
sudo apt update && sudo apt install lungo
```

**dnf (Fedora, RHEL):**

```sh
sudo tee /etc/yum.repos.d/lungo.repo >/dev/null <<'EOF'
[lungo]
name=lungo
baseurl=https://release.machinefabric.com/lungo-cli/rpm/$basearch
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=https://release.machinefabric.com/lungo-cli/keys/repo-signing-prod.asc
EOF
sudo dnf install lungo
```

**Windows:** the release's archive `lungo-<version>-windows-x86_64.zip`, listed with its
SHA-256 digest in the release manifest `https://release.machinefabric.com/lungo-cli/manifest`.
Unpack it and put the `lungo` directory on `PATH`.

**Cargo:** `cargo install lungo-cli` builds the same release from crates.io.

Every package is made from the release's archive of the command, which the release manifest
lists with its SHA-256 digest (see [trust and verification](../explanation/trust-and-verification.md)).
Built from the repository, `lungo` is a development build (see [the runtime](#the-runtime)).

## Configuration

The configuration is `lungo.toml` in the current directory, another file given with
`--config`, or the Lake project given with `--project` and the default settings (see
[configuration](configuration.md)). Giving both a file and `--project` is an error.

| Option | Meaning |
| --- | --- |
| `--config <FILE>` | A configuration file. Default: `./lungo.toml` when it exists. |
| `--project <DIR>` | The Lake project directory, instead of a configuration file. |
| `--root <MODULE>` | A root module. Repeatable. Adds to `root-modules`. |
| `--export <DECL>` | A declaration to export. Repeatable. |
| `--export-module <MODULE>` | A module whose declarations to export. Repeatable. |
| `--host-extern <KEY>` | An extern the application implements in the host language. Repeatable. |
| `--name <NAME>` | The program's name. |

These options are global: they may precede or follow the command.

## `generate`

```text
lungo generate [--<language>_out=DIR]... [--<language>_opt=KEY=VALUE[,KEY=VALUE]...]... [--runtime-dir DIR] [--wasi-sdk DIR] [--verify]
```

| Language | Output |
| --- | --- |
| `rust` | The Rust module `build.rs` generates, in `DIR/<name>/` (see [generated code](generated-code.md)). |
| `c` | A CMake project of a C library with the program's C API. |
| `go` | A Go package (cgo). |
| `python` | A Python project (scikit-build-core). |
| `swift` | A Swift package, with the C API for Objective-C. |
| `ts` | An npm package (ES module, type declarations, `program.wasm`). |
| any other name | The output of the plugin `lungo-gen-<name>` found on `PATH` (see [plugins](plugins.md)). |

[Generated packages](generated-packages.md) describes each language's package. `DIR`s on the
command line are relative to the current directory; the configuration's `out` directories
to the configuration file. Without any `--<language>_out`, `generate` writes the outputs the
configuration names (every language table with `out`, and `[rust]` with `out-dir`); with
none, it fails ([`LNG0107`](errors.md#lng0107)). `--<language>_opt` adds options to the
language's `options` and overrides them; `rust` has no options on the command line (its
settings are typed: see [configuration](configuration.md#rust)).

Each output directory belongs to lungo: it is replaced as a whole, and a directory lungo did
not create (one without its `build-info.json`) is never replaced
([`LNG0107`](errors.md#lng0107)). An output whose inputs (the Lean project, the toolchain,
lungo, the generator and its options) are unchanged, and which still holds exactly the files
it generated (none edited, removed or added since), is reused: `generate` prints
`up to date: <language> <DIR>` instead of `generated <language> into <DIR>`. The project is
analyzed once for every output (once more for `ts`, whose target is 32-bit WebAssembly).

Generated code is often committed, so that a package's users need neither Lean nor lungo.
`--verify` checks that it is current: it generates every output without writing, compares
each directory with it (every file but `build-info.json`), prints
`verified: <language> <DIR>` for each that matches, and fails with
[`LNG0110`](errors.md#lng0110) naming every file that changed, is missing or is extra.

| Option | Meaning |
| --- | --- |
| `--runtime-dir <DIR>` | A local lungo distribution to use instead of this release's (see [the runtime](#the-runtime)). |
| `--wasi-sdk <DIR>` | The wasi-sdk linking the TypeScript binding's WebAssembly. Default: `WASI_SDK_PATH`, else the pinned wasi-sdk release, downloaded into the cache and checked against its SHA-256 digest. |
| `--verify` | Compare instead of writing (above). |

## The runtime

Every language runs the program on the same runtime, prebuilt for every
[platform](platforms.md) and published with each lungo release: generated packages refer to
the release's runtime archives by URL and SHA-256 digest (the C package's CMake downloads and
checks it; the Go, Python, Swift and TypeScript support libraries carry it). A release of
`lungo` knows its runtime release from the manifest built into it.

A development build of `lungo` (built from source) knows no release. Generating a package
other than Rust then needs a *local distribution*: a directory laid out as a release
(`runtime/`, `wasm/`, `go/`, `python/`, `lungo-swift/`, `ts/`), built from the lungo
repository with `cargo run -p lungo-dist -- local --out DIR`, and given with `--runtime-dir
DIR`. With `--python PATH` it also holds lungo-py's wheel for this machine in `wheels/`, for pip
to find (`--find-links DIR/wheels`) when it builds a generated Python package. Without one the command fails with [`LNG0108`](errors.md#lng0108). The generated
packages then refer to that directory, so they build only on that machine.

```text
lungo runtime fetch [--target TRIPLE]
lungo runtime path [--target TRIPLE]
lungo runtime verify [--target TRIPLE]
```

| Command | Meaning |
| --- | --- |
| `runtime fetch` | Downloads the runtime archive for the target into the cache and unpacks it, after checking its SHA-256 digest; prints its directory. A cached archive that was checked is reused. The default target is the one `lungo` itself was built for: on Windows, the released command is built for the GNU ABI, so `x86_64-pc-windows-gnu`; name `--target x86_64-pc-windows-msvc` for the MSVC runtime. |
| `runtime path` | Prints the directory of the cached runtime for the target, or fails if it was not fetched. |
| `runtime verify` | Downloads the archive again and checks it. |

A download whose digest differs from the release's is discarded and reported as
[`LNG0109`](errors.md#lng0109). The runtime package holds `include/lungo.h`, the static and
shared libraries in `lib/`, `lib/cmake/lungo/lungoConfig.cmake` and
`lib/pkgconfig/lungo.pc`.

## Other commands

| Command | Output |
| --- | --- |
| `check` | Builds the project and translates it (Rust and C) without writing output; prints the toolchain, the number of modules, compiled declarations, externs and exports, and any warnings. |
| `inspect <DECL>` | For a declaration: Lean type, module, source (relative to the configuration), trust metadata, compiled signature, compiler auxiliaries, extern entry. |
| `ir <DECL>` | The Bridge IR of the declaration and of the auxiliaries the compiler derived from it. |
| `rust <DECL>` | The generated Rust of the declaration: its compiler-layer code and, when exported, its facade function. |
| `externs` | `externs.json`: every extern the program reaches and its Rust implementation. |
| `mappings` | `names.json`: every Lean-to-Rust name mapping. |
| `prepare` | Builds (or verifies) the worker for the project's toolchain; prints the Lean version, commit, worker identity and location. |
| `setup` | Installs the pinned toolchain with elan, materializes the dependencies locked in `lake-manifest.json`, and prepares the worker. |

Errors are printed as `error[LNGxxxx]: …` (see [errors](errors.md)) and the command exits
with status 1.

## Example

```text
$ lungo generate --go_out=gen/formal --python_out=gen/py
generated go into gen/formal
generated python into gen/py
$ lungo inspect Formal.apply
Formal.apply : Formal.Op → Formal.Sess → Option Formal.Sess
module: Formal.Session
source: lean/Formal/Session.lean
exported: yes
axioms:
depends on sorry: false
unsafe dependencies:
partial dependencies:
extern dependencies: lean_nat_add
compiled: (u8, obj) -> tobj in module Formal.Session
compiler auxiliaries: Formal.apply._boxed
```
