# lean2rust

lean2rust turns an ordinary Lake project into Rust code generated at Cargo build time. Lean's
own frontend elaborates and kernel-checks the project, and Lean's compiler compiles it. A
worker built against the project's exact Lean toolchain hands the compiled program to a Rust
backend as a versioned Bridge IR (BIR). The backend reproduces it on a Rust port of Lean's
runtime and wraps it in an idiomatic Rust facade.

The architecture is specified in [docs/DESIGN.md](docs/DESIGN.md). How this implementation
realizes it for Lean 4.34.1 is described in [docs/IMPLEMENTATION.md](docs/IMPLEMENTATION.md).

## Using it

A crate keeps an untouched Lake project next to its sources:

```text
my-crate/
├── Cargo.toml
├── build.rs
├── src/lib.rs
└── lean/                 # a normal Lake project: lean-toolchain, lakefile, lake-manifest.json
    └── Formal/Session.lean
```

`build.rs` describes what to compile and expose:

```rust
use lean2rust_build::{Config, Mode};

fn main() -> lean2rust_build::Result<()> {
    Config::new("lean")
        .root_module("Formal.Session")
        .export_module("Formal")
        .mode(Mode::PureRust)
        .compile()
}
```

and the crate includes the generated code from `OUT_DIR`:

```rust
pub mod formal {
    include!(concat!(env!("OUT_DIR"), "/lean2rust/formal.rs"));
}
```

The Lean definitions become ordinary Rust:

```rust
use formal::{Op, Sess};
let s = formal::apply(Op::Open, Sess { is_open: false, count: 0u64.into() });
```

Dependencies: `lean2rust` (the runtime and facade support) as a normal dependency, and
`lean2rust-build` as a build dependency. [examples/session](examples/session) is the complete
example from the design document.

### What gets generated

`$OUT_DIR/lean2rust/` contains:

| File | Contents |
| --- | --- |
| `formal.rs` | The aggregate include target: the public facade and the compiler layer |
| `modules/*.rs` | The compiled code of every module in the program's executable closure |
| `names.json` | Every Lean-name to Rust-name mapping (items, constructors, fields) |
| `externs.json` | Every extern the program reaches and how it is implemented |
| `sources.json` | Source locations of the compiled declarations |
| `manifest.json` | Exports, their Rust paths and trust metadata |
| `build-info.json` | The build key and inputs, for incremental rebuilds |

Every exported declaration also has static metadata: `formal::__meta::declaration("Formal.apply")`
reports its module, Lean type, source range, axioms, and `sorry`/`unsafe`/`partial`/extern
dependencies.

### Configuration

`Config` methods (and the equivalent kebab-case keys of `lean2rust.toml`, which
`cargo lean2rust` reads):

| Setting | Meaning |
| --- | --- |
| `root_module` / `root-modules` | Modules whose executable code is compiled (required) |
| `export` / `exports` | Declarations that receive public Rust facades |
| `export_module` / `export-modules` | Modules whose public declarations receive facades |
| `mode` | `PureRust` (default) or `LeanOracle` |
| `rust_extern(symbol, path)` / `rust-externs` | A Rust implementation of an `@[extern]` symbol |
| `deny_sorry` (default on), `deny_axioms`, `deny_unsafe` | Trust policies for exported code |
| `embed_sources` | Embed the local Lean sources in the generated code |
| `facade_namespace`, `output_name`, `output_dir` | Placement of the generated code |
| `worker_timeout`, `hermetic`, `hermetic_worker_cache` | Worker supervision and isolation |
| `toolchain_dir`, `install_toolchain` | Toolchain resolution (builds never install by default) |

### Modes

- **PureRust** (the default) runs generated Rust on the `lean2rust` runtime. No Lean runtime
  and no C code is linked.
- **LeanOracle** runs Lean's official C backend and runtime behind the same facade. It is a
  reference oracle for differential testing, needs the host's Lean toolchain at link time,
  and requires the GNU ABI on Windows.

### Host integration

Lean `@[extern "symbol"]` declarations resolve, in order, to a Lean definition that
`@[export]`s the symbol, to a runtime primitive, and then to an application function mapped with
`rust_extern`. An unresolved symbol is a build error naming the declaration, its type, and
the representation it needs. Application externs take and return facade types:

```rust
// Lean: @[extern "host_lookup"] opaque hostLookup (key : @& String) : Option Nat
pub fn lookup(key: String) -> Option<lean2rust::Nat> { /* ... */ }
```

Closures cross the boundary in both directions (`LeanClosure::call` and
`LeanClosure::from_fn`); see [compiler-tests/host](compiler-tests/host).

## Command line

`cargo install --path crates/cargo-lean2rust` provides `cargo lean2rust`, which runs the same
pipeline as `build.rs`:

```text
cargo lean2rust check              # build and translate, without writing output
cargo lean2rust build              # generate into target/lean2rust/out
cargo lean2rust inspect Formal.apply
cargo lean2rust ir Formal.apply    # Bridge IR, with the compiler's auxiliaries
cargo lean2rust rust Formal.apply  # generated Rust
cargo lean2rust externs            # externs.json
cargo lean2rust mappings           # names.json
cargo lean2rust prepare            # build the worker for the project's toolchain
cargo lean2rust setup              # install the pinned toolchain and locked dependencies
```

## Requirements

- Rust 1.89 or later.
- [elan](https://github.com/leanprover/elan) with the toolchain the Lake project pins
  (`leanprover/lean4:v4.34.1` is supported). Builds never download toolchains: install them
  explicitly (`elan toolchain install leanprover/lean4:v4.34.1` or `cargo lean2rust setup`).
- A committed `lake-manifest.json`. Builds never resolve or update dependencies.

## Repository

| Path | Contents |
| --- | --- |
| `worker/` | The Lean worker: reads Lean's compiler output and emits Bridge IR |
| `crates/lean2rust-bir` | The Bridge IR data model and its independent verifier |
| `crates/lean2rust-protocol` | The versioned worker protocol |
| `crates/lean2rust-runtime` | The Rust port of Lean's runtime and its primitives |
| `crates/lean2rust` | Facade types and runtime re-export used by generated code |
| `crates/lean2rust-codegen` | The Rust backend: compiler layer and facade generator |
| `crates/lean2rust-build` | Build-script integration: toolchain, Lake, worker, output |
| `crates/cargo-lean2rust` | The `cargo lean2rust` command |
| `examples/session` | The design document's example |
| `compiler-tests/conformance` | Lean programs compared against Lean's native backend |
| `compiler-tests/facade` | A Lean library compared between PureRust and LeanOracle |
| `compiler-tests/host` | Rust callbacks, custom syntax and macros, link checks |
| `compiler-tests/gates` | Release gates of the build pipeline |
| `compiler-tests/stdlib-corpus` | Lean's whole executable `Init`/`Std`, generated as Rust (checked in CI) |
| `runtime-tests/` | Inventories of the toolchain's native interface |

## Testing

```sh
elan toolchain install leanprover/lean4:v4.34.1
cargo test --workspace
```

The suite includes:
- a differential test of every conformance program against the executable Lake builds with
  Lean's native backend;
- property tests comparing PureRust with LeanOracle call by call;
- a check that every `@[extern]` symbol of the toolchain is classified and every BIR
  instruction the toolchain emits is exercised;
- the release gates of design §49;
- the whole executable `Init` and `Std` library translated, verified and (in CI,
  `cargo check --manifest-path compiler-tests/stdlib-corpus/Cargo.toml`) type-checked.
