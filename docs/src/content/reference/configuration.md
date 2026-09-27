---
title: "Configuration"
description: "Every lungo.toml key and lungo_build::Builder setting, with its default."
---

lungo is configured in two places that describe the same settings:

- `lungo.toml`, read by the [`lungo` command](lungo-cli.md), for every output language;
- a `lungo_build::Builder` in a Cargo `build.rs`, for the Rust output
  ([`compile_lean(project)`](#entry-points) is `configure().compile_lean(project)`).

Every setting has a default. Unknown keys are errors ([`LNG0107`](errors.md#lng0107)).

```toml
# lungo.toml
project = "lean"

[lean]
root-modules = ["Formal.Session"]
host-externs = ["host_log"]

[rust]
out-dir = "src/generated"
type-attributes = [
  { path = "Formal", attribute = "#[derive(serde::Serialize, serde::Deserialize)]" },
]
rust-externs = { host_log = "crate::host::log" }

[go]
out = "gen/formal"
options = { package = "formal" }

[plugins.kotlin]
out = "gen/kotlin"
```

```rust
// build.rs
fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        .root_module("Formal.Session")
        .type_attribute("Formal", "#[derive(serde::Serialize, serde::Deserialize)]")
        .rust_extern("host_log", "crate::host::log")
        .compile_lean("lean")
}
```

## The file

| Key | Meaning |
| --- | --- |
| `project` | The Lake project directory. Required. |
| `[lean]` | The settings every language shares: what is compiled and exported, the host externs, the trust policy, the program's name, the worker, the toolchain. |
| `[rust]` | The Rust generator's settings. |
| `[c]`, `[go]`, `[python]`, `[swift]`, `[ts]` | A built-in generator's output directory and [options](#generator-options). |
| `[plugins.<name>]` | A [plugin](plugins.md)'s output directory and options. `<name>` is lowercase letters, digits and `-`, and not the name of a built-in generator. |

Paths in the file (`project`, `out`, `out-dir`, `toolchain-dir`) are relative to the file's
directory. `lungo` keeps scratch files in `.lungo/` next to the file; add it to
`.gitignore`.

## Program

`[lean]`, or the `Builder` methods:

| Builder | TOML key | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `root_module(m)` | `root-modules` | list of module names | the default targets' roots | Modules of the project whose code is compiled, with everything they import. Each must belong to the project's root package. |
| `export(d)` | `exports` | list of declaration names | `[]` | Declarations that receive public functions. |
| `export_module(m)` | `export-modules` | list of module names | `[]` | Modules (with their submodules) whose public definitions receive public functions. |
| `rust_extern(key, path)` | `host-externs` | list of extern keys | `[]` | Externs the application implements in the host language (see [how to call host code from Lean](../how-to/call-host-code-from-lean.md)). `rust_extern` also maps the key to its Rust implementation (`[rust] rust-externs`). |
| `name(n)` | `name` | string | the Lake package's name | The program's name: of the generated module, package and symbol prefix. Letters, digits, `_` and `-`, not starting with a digit or `-`. |

An extern key is the symbol of `@[extern "symbol"]`, or the declaration's name for the other
extern forms.

Without `root_module`, the roots are what `lake build` builds: the root modules of every
default target of the root package (`defaultTargets` in `lakefile.toml`, `@[default_target]`
in `lakefile.lean`), which are Lean libraries (their `roots`) or Lean executables (their
`root`). A package without default targets needs `root_module`
([`LNG0301`](errors.md#lng0301)).

Without `export` and `export_module`, the root modules and their submodules are exported.
From an exported module, definitions are exported unless they are private, internal
(`_`-prefixed components), instances or their methods, compiler-generated (recursors,
matchers, `noConfusion`, projections, constructors, structural auxiliaries), syntax node
kinds (`syntax`, `notation`, `macro`), or `meta` definitions.

## Trust policy

`[lean]`:

| Builder | TOML key | Default | Rejects an export that… |
| --- | --- | --- | --- |
| `deny_sorry(b)` | `deny-sorry` | `true` | depends on `sorry` |
| `deny_axioms(b)` | `deny-axioms` | `false` | depends on an axiom other than `propext`, `Classical.choice`, `Quot.sound` |
| `deny_unsafe(b)` | `deny-unsafe` | `false` | has an `unsafe` definition in its executable closure (outside the Lean toolchain) |

Violations fail the build with [`LNG0601`](errors.md#lng0601).

## Worker

`[lean]`:

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `worker_timeout(d)` | `worker-timeout` | none | Wall-clock seconds after which the worker's processes are killed ([`LNG0304`](errors.md#lng0304)). |
| `worker_cpu_limit(d)` | `worker-cpu-limit` | none | Processor seconds the worker may use ([`LNG0305`](errors.md#lng0305)). |
| `worker_memory_limit(bytes)` | `worker-memory-limit` | none | Memory the worker may use. Enforced on Linux and Windows; an error ([`LNG0104`](errors.md#lng0104)) elsewhere. |
| `hermetic(b)` | `hermetic` | `false` | Run the worker with only `PATH`, `HOME`, `USERPROFILE`, `SystemRoot`, `SYSTEMROOT`, `TEMP`, `TMP`, `TMPDIR`, `LANG`, `LC_ALL`, `ELAN_HOME`, `LOCALAPPDATA`, `APPDATA` from the environment, plus Lake's environment for the project. |
| `hermetic_worker_cache(b)` | `hermetic-worker-cache` | `false` | Keep the compiled worker beside the build output instead of in the shared cache. |
| `lean_option(name, value)` | `lean-options` | `{}` | Lean options in effect while the worker loads the project and runs Lean metaprograms. |
| `max_errors(n)` | `max-errors` | `0` | Report at most `n` errors from the worker; `0` reports all. |

## Toolchain

`[lean]`:

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `toolchain_policy(p)` | `install-toolchain` | `Strict` / `false` | `Install` (`true`) installs a missing pinned toolchain with elan. `Strict` (`false`) fails with [`LNG0103`](errors.md#lng0103). |
| `toolchain_dir(dir)` | `toolchain-dir` | elan's installation | Use the toolchain installed in `dir`. |

## Rust

`[rust]`, or the `Builder` methods:

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `mode(m)` | `mode` | `pure-rust` | `pure-rust` generates Rust on the lungo runtime. `lean-oracle` runs Lean's native backend behind the same API (see [platforms](platforms.md)). |
| `rust_extern(key, path)` | `rust-externs` | `{}` | The Rust function implementing each host extern: key → Rust path. Its keys are exactly the `host-externs` of `[lean]`. |
| `out_dir(d)` | `out-dir` | `$OUT_DIR/lungo` | The directory receiving `<name>/`. In `build.rs`, relative to the Cargo package, and `include_lean!` finds only the default (use `include!` for another). For `lungo generate`, the Rust output (`--rust_out` overrides it). |
| `facade_namespace(n)` | `facade-namespace` | first component of the first root module | The Lean namespace placed at the root of the generated module. Names outside it are placed under `_root_`. |
| `embed_sources(b)` | `embed-sources` | `false` | Embed the local Lean sources, available through `__meta::source`. |
| `emit_rerun_if_changed(b)` | `emit-rerun-if-changed` | whether the build runs under Cargo | Print `cargo::rerun-if-changed` for every input, and `cargo::rerun-if-env-changed` for the [environment variables](#environment-variables). |

One build script may compile several Lean projects; each generates its own module, and two
that would generate the same module are an error ([`LNG0107`](errors.md#lng0107)). Give one
of them another `name`.

### Shaping the generated code

These settings select generated items by *path*. The path `.` selects every item; any
other path selects the Lean name it spells and every name in it as a namespace: `Formal`
selects `Formal.Sess` and `Formal.Sess.isOpen`, but `Formal.Se` selects neither. Fields are
named as in [`names.json`](generated-code.md#namesjson): `<Structure>.<field>`,
`<Constructor>.<binder>`, or `<Constructor>#<index>`. A path other than `.` that selects
nothing is an error ([`LNG0107`](errors.md#lng0107)): it cannot have any effect.

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `type_attribute(path, attr)` | `type-attributes` | `[]` | Adds the Rust attribute `attr` to every generated struct and enum `path` selects. |
| `struct_attribute(path, attr)` | `struct-attributes` | `[]` | The same, for structs only. |
| `enum_attribute(path, attr)` | `enum-attributes` | `[]` | The same, for enums only. |
| `field_attribute(path, attr)` | `field-attributes` | `[]` | Adds `attr` to every field of a generated type `path` selects. |
| `skip_debug(paths)` | `skip-debug` | `[]` | Generates the selected types without `#[derive(Debug)]`, for the application to implement `Debug`. |
| `disable_comments(paths)` | `disable-comments` | `[]` | Generates the selected types and functions without documentation comments. |
| `extern_type(lean, path)` | `extern-types` | `{}` | Uses the existing Rust type at `path` for the Lean type `lean` instead of generating one. |

In TOML, attribute settings are lists of `{ path, attribute }` tables, `skip-debug` and
`disable-comments` lists of paths, and `extern-types` a table from Lean type to Rust path.

Attributes are written as in Rust source, `#[…]`, and are placed after the derives lungo
generates (`Clone`, `Debug`, and `PartialEq`, `Eq`, `Hash` where every field has them), so
they may be `#[derive(…)]`s of further traits or attributes of those derives, such as
`#[serde(…)]`. The `serde` feature of the `lungo` crate implements `serde` for the
[facade types](type-mapping.md#serde).

An extern type implements `lungo::LeanType` for the backend in use, has the Lean type's
parameters, and represents its values as the Lean type does; in practice it is the type
another lungo build generated for the same Lean type. It is not generated, and types
containing it derive only `Clone` and `Debug`. See
[How to share types between Lean projects](../how-to/share-types-between-projects.md).

## Generator options

A language's table holds its output directory, `out`, and its options, `options`, a table of
strings. `--<language>_opt=KEY=VALUE` on the command line adds to (and overrides) them. A
generator rejects options it does not know.

| Language | Option | Default | Meaning |
| --- | --- | --- | --- |
| `c` | — | | |
| `go` | `package` | the program's C identifier, lowercased | The Go package name. |
| `python` | `package` | the program's C identifier, lowercased | The import name. |
| `python` | `distribution` | `package` with `_` as `-` | The distribution (PyPI) name. |
| `python` | `version` | `0.1.0` | The distribution's version. |
| `swift` | `module` | the program's name in UpperCamelCase | The Swift module; the C target is `<module>Program`. |
| `ts` | `package` | the program's name, lowercased | The npm package name. |
| `ts` | `version` | `0.1.0` | The package's version. |

See [generated packages](generated-packages.md) for what each generator writes.

## Environment variables

| Variable | Meaning |
| --- | --- |
| `ELAN_HOME` | Where elan keeps toolchains (default `~/.elan`). |
| `LUNGO_CACHE_DIR` | Root of lungo's cache: compiled workers in `workers/`, downloaded runtimes in `runtime/`, the wasi-sdk in `wasi-sdk/`. Defaults: `$XDG_CACHE_HOME/lungo`, `~/.cache/lungo`, or `%LOCALAPPDATA%\lungo`. |
| `WASI_SDK_PATH` | A wasi-sdk to link the TypeScript binding's WebAssembly with, instead of the pinned release lungo downloads. |

## Entry points

| Function | Use |
| --- | --- |
| `compile_lean(project)` | In `build.rs`, with the default configuration. |
| `configure()` | A `Builder` with the default configuration. |
| `Builder::compile_lean(self, project)` | In `build.rs`. Generates into `<out_dir>/<name>` and prints Cargo directives: `rerun-if-changed` (see `emit_rerun_if_changed`) and, in `LeanOracle` mode, link directives. `project` is relative to the Cargo package. |
| `Builder::run(&self, project, &Environment)` | The same pipeline for a given `Environment`; returns a `BuildOutcome`. |
| `Builder::analyze` / `Builder::generate` | The two halves of `run`: the worker's analysis, then code generation, without publishing. |
| `Builder::from_options(lean, rust)` | A `Builder` of a `LeanOptions` (`[lean]`) and a `RustOptions` (`[rust]`). |
