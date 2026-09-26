# Configuration

A patina build is configured by a `patina_build::Builder`, made with
`patina_build::configure()` in `build.rs`, or read from the `[build]` table of a
`patina.toml` by `cargo patina`. Both describe the same settings; the TOML keys are the field
names in kebab case. Every setting has a default, and `patina_build::compile_lean(project)`
is `configure().compile_lean(project)`.

```rust
// build.rs
fn main() -> patina_build::Result<()> {
    patina_build::configure()
        .root_module("Formal.Session")
        .type_attribute("Formal", "#[derive(serde::Serialize, serde::Deserialize)]")
        .compile_lean("lean")
}
```

```toml
# patina.toml
project = "lean"

[build]
root-modules = ["Formal.Session"]
type-attributes = [
  { path = "Formal", attribute = "#[derive(serde::Serialize, serde::Deserialize)]" },
]
```

In `patina.toml`, `project` is the Lake project directory, relative to the Cargo package;
`[build]` may be omitted. Unknown keys are errors ([`PTN0107`](errors.md#ptn0107)).

## Program

| Builder | TOML key | Type | Default | Meaning |
| --- | --- | --- | --- | --- |
| `root_module(m)` | `root-modules` | list of module names | the default targets' roots | Modules of the project whose code is compiled, with everything they import. Each must belong to the project's root package. |
| `export(d)` | `exports` | list of declaration names | `[]` | Declarations that receive public Rust functions. |
| `export_module(m)` | `export-modules` | list of module names | `[]` | Modules (with their submodules) whose public definitions receive public Rust functions. |
| `mode(m)` | `mode` | `pure-rust` or `lean-oracle` | `pure-rust` | `PureRust` generates Rust on the patina runtime. `LeanOracle` runs Lean's native backend behind the same API (see [platforms](platforms.md)). |
| `rust_extern(symbol, path)` | `rust-externs` | table: symbol → Rust path | `{}` | An application function implementing an `@[extern "symbol"]` declaration. |

Without `root_module`, the roots are what `lake build` builds: the root modules of every
default target of the root package (`defaultTargets` in `lakefile.toml`, `@[default_target]`
in `lakefile.lean`), which are Lean libraries (their `roots`) or Lean executables (their
`root`). A package without default targets needs `root_module`
([`PTN0301`](errors.md#ptn0301)).

Without `export` and `export_module`, the root modules and their submodules are exported.
From an exported module, definitions are exported unless they are private, internal
(`_`-prefixed components), instances or their methods, compiler-generated (recursors,
matchers, `noConfusion`, projections, constructors, structural auxiliaries), syntax node
kinds (`syntax`, `notation`, `macro`), or `meta` definitions.

## Shaping the generated code

These settings select generated items by *path*. The path `.` selects every item; any
other path selects the Lean name it spells and every name in it as a namespace: `Formal`
selects `Formal.Sess` and `Formal.Sess.isOpen`, but `Formal.Se` selects neither. Fields are
named as in [`names.json`](generated-code.md#namesjson): `<Structure>.<field>`,
`<Constructor>.<binder>`, or `<Constructor>#<index>`. A path other than `.` that selects
nothing is an error ([`PTN0107`](errors.md#ptn0107)): it cannot have any effect.

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

Attributes are written as in Rust source, `#[…]`, and are placed after the derives patina
generates (`Clone`, `Debug`, and `PartialEq`, `Eq`, `Hash` where every field has them), so
they may be `#[derive(…)]`s of further traits or attributes of those derives, such as
`#[serde(…)]`. The `serde` feature of the `patina` crate implements `serde` for the
[facade types](type-mapping.md#serde).

An extern type implements `patina::LeanType` for the backend in use, has the Lean type's
parameters, and represents its values as the Lean type does; in practice it is the type
another patina build generated for the same Lean type. It is not generated, and types
containing it derive only `Clone` and `Debug`. See
[How to share types between Lean projects](../how-to/share-types-between-projects.md).

## Trust policy

| Builder | TOML key | Default | Rejects an export that… |
| --- | --- | --- | --- |
| `deny_sorry(b)` | `deny-sorry` | `true` | depends on `sorry` |
| `deny_axioms(b)` | `deny-axioms` | `false` | depends on an axiom other than `propext`, `Classical.choice`, `Quot.sound` |
| `deny_unsafe(b)` | `deny-unsafe` | `false` | has an `unsafe` definition in its executable closure (outside the Lean toolchain) |

Violations fail the build with [`PTN0601`](errors.md#ptn0601).

## Output

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `name(n)` | `name` | the Lake package's name | The generated module's name: `patina::include_lean!("<name>")` includes it. Letters, digits, `_` and `-`. |
| `out_dir(d)` | `out-dir` | `$OUT_DIR/patina` (`cargo patina`: `target/patina/out`) | The directory receiving `<name>/`; relative to the Cargo package. `include_lean!` finds only the default; use `include!` for another. |
| `facade_namespace(n)` | `facade-namespace` | first component of the first root module | The Lean namespace placed at the root of the generated module. Names outside it are placed under `_root_`. |
| `embed_sources(b)` | `embed-sources` | `false` | Embed the local Lean sources, available through `__meta::source`. |
| `emit_rerun_if_changed(b)` | `emit-rerun-if-changed` | whether the build runs under Cargo | Print `cargo::rerun-if-changed` for every input, and `cargo::rerun-if-env-changed` for the [environment variables](#environment-variables). |

One build script may compile several Lean projects; each generates its own module, and two
that would generate the same module are an error ([`PTN0107`](errors.md#ptn0107)). Give one
of them another `name`.

## Worker

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `worker_timeout(d)` | `worker-timeout` | none | Wall-clock seconds after which the worker's processes are killed ([`PTN0304`](errors.md#ptn0304)). |
| `worker_cpu_limit(d)` | `worker-cpu-limit` | none | Processor seconds the worker may use ([`PTN0305`](errors.md#ptn0305)). |
| `worker_memory_limit(bytes)` | `worker-memory-limit` | none | Memory the worker may use. Enforced on Linux and Windows; an error ([`PTN0104`](errors.md#ptn0104)) elsewhere. |
| `hermetic(b)` | `hermetic` | `false` | Run the worker with only `PATH`, `HOME`, `USERPROFILE`, `SystemRoot`, `SYSTEMROOT`, `TEMP`, `TMP`, `TMPDIR`, `LANG`, `LC_ALL`, `ELAN_HOME`, `LOCALAPPDATA`, `APPDATA` from the environment, plus Lake's environment for the project. |
| `hermetic_worker_cache(b)` | `hermetic-worker-cache` | `false` | Keep the compiled worker beside the build output instead of in the shared cache. |
| `lean_option(name, value)` | `lean-options` | `{}` | Lean options in effect while the worker loads the project and runs Lean metaprograms. |
| `max_errors(n)` | `max-errors` | `0` | Report at most `n` errors from the worker; `0` reports all. |

## Toolchain

| Builder | TOML key | Default | Meaning |
| --- | --- | --- | --- |
| `toolchain_policy(p)` | `install-toolchain` | `Strict` / `false` | `Install` (`true`) installs a missing pinned toolchain with elan. `Strict` (`false`) fails with [`PTN0103`](errors.md#ptn0103). |
| `toolchain_dir(dir)` | `toolchain-dir` | elan's installation | Use the toolchain installed in `dir`. |

## Environment variables

| Variable | Meaning |
| --- | --- |
| `ELAN_HOME` | Where elan keeps toolchains (default `~/.elan`). |
| `PATINA_CACHE_DIR` | Root of the shared worker cache; workers are kept in `<dir>/workers`. Defaults: `$XDG_CACHE_HOME/patina/workers`, `~/.cache/patina/workers`, or `%LOCALAPPDATA%\patina\workers`. |

## Entry points

| Function | Use |
| --- | --- |
| `compile_lean(project)` | In `build.rs`, with the default configuration. |
| `configure()` | A `Builder` with the default configuration. |
| `Builder::compile_lean(self, project)` | In `build.rs`. Generates into `<out_dir>/<name>` and prints Cargo directives: `rerun-if-changed` (see `emit_rerun_if_changed`) and, in `LeanOracle` mode, link directives. `project` is relative to the Cargo package. |
| `Builder::run(&self, project, &Environment)` | The same pipeline for a given `Environment`; returns a `BuildOutcome`. |
| `Builder::analyze` / `Builder::generate` | The two halves of `run`: the worker's analysis, then code generation, without publishing. |
| `ProjectFile::load(path)` | Reads a `patina.toml`: `project` and `build`. |
