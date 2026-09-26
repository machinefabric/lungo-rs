# Generated code

A build publishes the module `<name>` into `<out_dir>/<name>`, by default
`$OUT_DIR/lungo/<name>`, where `<name>` is the Lake package's name unless configured (see
[configuration](configuration.md#output)). The directory is replaced atomically: it holds
either the previous complete output or the new one.

## Files

| File | Contents |
| --- | --- |
| `<name>.rs` | The aggregate include target: the public facade and the compiler layer. |
| `modules/<Module>.rs` | Compiled code of one module of the program, included by the aggregate. File names replace `.` by `-` and other non-identifier characters by `+<hex>+`. |
| `names.json` | Name mappings. |
| `externs.json` | Extern resolutions. |
| `sources.json` | Source locations of compiled declarations. |
| `manifest.json` | Versions, modules and exports. |
| `build-info.json` | Build key and inputs, for incremental rebuilds. Not compiled into the crate. |
| `native/liblungo_oracle_<name>.a` | `LeanOracle` mode only: Lean's native code for the project (`-` in `<name>` becomes `_`). |

Paths recorded in these files are relative to the Cargo package; files outside it are written
`<lean>/…` (toolchain sources) or `<package>/…` (Lake dependencies). No file except
`build-info.json` records machine-specific paths.

## The aggregate module

`lungo::include_lean!("<name>")` includes the aggregate from the default output directory,
where it expands to `include!(concat!(env!("OUT_DIR"), "/lungo/<name>/<name>.rs"))`. It is
usually the only item of a Rust module:

```rust
pub mod formal {
    lungo::include_lean!("formal");
}
```

| Item | Contents |
| --- | --- |
| types and functions | The facade: one item per exported declaration and per type reachable from exports and `rust_extern` functions, except [extern types](configuration.md#shaping-the-generated-code). Declarations in the facade namespace are at the root; `A.B.f` is `a::b::f`; other names are under `_root_`. |
| `__meta` | Metadata; see below. |
| `__lean_main()`, `__lean_main_with(args)` | Present when a root module defines `main`. Run the Lean program with the process arguments (or `args`) and return its exit code. |
| `__lng`, `__opaque`, `__oracle` | Hidden: the compiler layer, marker types of opaque values, the `LeanOracle` backend. Not a stable interface. |

Every facade function initializes the program's modules on first use. Its doc comment
records the Lean name, type and source location, unless `disable_comments` selects it.
Generated types derive `Clone`, `Debug` (unless `skip_debug` selects them), and `PartialEq`,
`Eq` and `Hash` where all their fields implement them, followed by the configured
attributes.

## `__meta`

| Item | Type |
| --- | --- |
| `LEAN_VERSION`, `LEAN_GITHASH` | `&str`: the toolchain the code was compiled with. |
| `BIR_VERSION` | `u32` |
| `GENERATOR_VERSION` | `&str`: the lungo version. |
| `declaration(lean_name)` | `Option<&'static lungo::DeclarationInfo>` for an exported declaration. |
| `declarations()` | `&'static [DeclarationInfo]`, sorted by Lean name. |
| `module_source(module)`, `source(lean_name)` | `Option<&'static str>`: embedded Lean source. Present with `embed_sources`. |

`DeclarationInfo`:

| Field | Type | Meaning |
| --- | --- | --- |
| `lean_name` | `&str` | Fully qualified Lean name. |
| `module` | `&str` | Defining module. |
| `source_file` | `Option<&str>` | Source file, package-relative. |
| `range` | `Option<SourceRange>` | One-based `start`/`end` `SourcePosition { line, column }`. |
| `lean_type` | `&str` | The Lean type, pretty-printed. |
| `compiled_signature` | `&str` | Runtime representation, e.g. `(u8, obj) -> tobj`; `@&` marks borrowed parameters. |
| `rust_path` | `&str` | Path of the Rust item within the aggregate module. |
| `trust` | `ExportTrust` | See below. |

`ExportTrust`:

| Field | Meaning |
| --- | --- |
| `axioms` | Axioms the definition depends on, excluding `sorryAx`. |
| `depends_on_sorry` | Whether it depends on `sorry`. |
| `unsafe_dependencies` | `unsafe` constants in the executable closure: the declaration itself and those outside the Lean toolchain. |
| `partial_dependencies` | `partial` constants, with the same scope. |
| `extern_dependencies` | Extern symbols the compiled code calls. |

## `names.json`

An array of records, one per public Rust name:

| Field | Meaning |
| --- | --- |
| `lean_name` | The Lean name. Fields of structures are `<Structure>.<field>`; other constructor arguments `<Constructor>.<binder>`, or `<Constructor>#<index>` for unnamed binders. |
| `kind` | `type`, `function`, `constructor`, `field`, or `extern type` for a type the application provides with `extern_type`. |
| `rust_path` | The Rust path within the aggregate module (for an extern type, the configured path); fields are `<constructor path>.<field>`, e.g. `Sess.is_open`, `Shape::Rect.width`, `Pixel.0`. |
| `renamed` | Whether the Rust identifier differs from the last component of the Lean name. |

Items come first, in placement order, then constructors and fields.

## `externs.json`

An array, one record per extern declaration in the program:

| Field | Meaning |
| --- | --- |
| `declaration` | The Lean extern declaration. |
| `key` | The symbol (or the declaration's name for `adhoc` externs). |
| `resolution` | `lean_export` (a Lean `@[export]` definition), `runtime` (a lungo primitive), or `application` (a `rust_extern` function). |
| `implementation` | The Lean definition, runtime path, or Rust path. |
| `lean_type` | The Lean type, when the declaration has a source-level constant. |
| `source` | Source location, when known. |

## `sources.json`

An array of `{ lean_name, file, start, end }`, with `start`/`end` as `[line, column]` or
`null`, for every source-level declaration with compiled code.

## `manifest.json`

| Field | Meaning |
| --- | --- |
| `lean_version`, `lean_githash` | The toolchain. |
| `bir_version`, `generator_version`, `runtime_abi` | lungo versions. |
| `modules` | Modules of the program, in initialization order. |
| `declarations` | Number of compiled declarations. |
| `exports` | Per export: `lean_name`, `module`, `rust_path`, `lean_type`, `source`, `trust` (as in `ExportTrust`). |

## `build-info.json`

| Field | Meaning |
| --- | --- |
| `build_key` | Digest of everything the output depends on besides input files: toolchain identity, configuration, target, lungo and worker versions, generator binary. |
| `lean_version`, `lean_githash`, `bir_version`, `adapter_version`, `lungo_version`, `runtime_abi`, `worker_identity` | Versions. |
| `input_digests` | Input files (relative to the Lean project) and their content digests. |
| `project` | The Lean project directory, relative to the package. |
| `link_directives` | Cargo link directives (empty in `PureRust` mode). |

## Versioning

Generated code asserts the runtime ABI it was generated for at compile time
(`lungo::__runtime::assert_abi::<N>()`), so output generated by a different lungo
version fails to compile rather than running against an incompatible runtime.
