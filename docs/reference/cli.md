# `cargo lungo`

`cargo lungo` runs the same pipeline as `build.rs` for a Cargo package and reports on it.
It is installed from the repository:

```sh
cargo install --path crates/cargo-lungo
```

```text
cargo lungo [OPTIONS] <COMMAND>
```

## Configuration

The configuration is `<package>/lungo.toml`, another file given with `--config`, or the
Lake project given on the command line with `--project` and the default settings. A
configuration file names the Lake project and, in its optional `[build]` table, the build
settings (see [configuration](configuration.md)):

```toml
project = "lean"

[build]
export-modules = ["Formal"]
```

Command-line `--root`, `--export`, `--export-module` and `--oracle` add to the file's settings.

| Option | Meaning |
| --- | --- |
| `--package-dir <DIR>` | The Cargo package directory. Default: the current directory. |
| `--config <FILE>` | A configuration file. Default: `<package>/lungo.toml` when it exists. |
| `--project <DIR>` | The Lake project directory, relative to the package, instead of a configuration file. |
| `--root <MODULE>` | A root module. Repeatable. Default: the roots of the Lake package's default targets. |
| `--export <DECL>` | A declaration to export. Repeatable. |
| `--export-module <MODULE>` | A module whose declarations to export. Repeatable. |
| `--oracle` | Use `LeanOracle` mode. |
| `--out-dir <DIR>` | Where `build` publishes the module, as `<DIR>/<name>`. Default: the configuration's `out-dir`, else `<package>/target/lungo/out`. |

Giving both a configuration file and `--project` is an error. Scratch files go to
`<package>/target/lungo/work`.

## Commands

| Command | Output |
| --- | --- |
| `check` | Builds and translates the project without publishing; prints the toolchain, the number of modules, compiled declarations, externs and exports, and any warnings. |
| `build` | Generates the module into `<out-dir>/<name>`; prints `generated <dir>`, or `up to date: <dir>` when the previous output is reused. |
| `inspect <DECL>` | For a declaration: Lean type, module, source (relative to the package), trust metadata, compiled signature, compiler auxiliaries, extern entry. |
| `ir <DECL>` | The Bridge IR of the declaration and of the auxiliaries the compiler derived from it. |
| `rust <DECL>` | The generated Rust of the declaration: its compiler-layer code and, when exported, its facade function. |
| `externs` | `externs.json`: every extern the program reaches and its implementation. |
| `mappings` | `names.json`: every Lean-to-Rust name mapping. |
| `prepare` | Builds (or verifies) the worker for the project's toolchain; prints the Lean version, commit, worker identity and location. |
| `setup` | Installs the pinned toolchain with elan, materializes the dependencies locked in `lake-manifest.json`, and prepares the worker. The only command that downloads anything. |

Errors are printed as `error[LNGxxxx]: …` (see [errors](errors.md)) and the command exits
with status 1.

## Example

```text
$ cargo lungo inspect Formal.apply
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
