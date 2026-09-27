---
title: "Architecture"
description: "How lungo's pieces fit together: why Lean compiles the program before lungo translates it, and how one program reaches every language on one runtime."
---

lungo makes Lean code available to programs in Rust, C, Go, Python, Swift, Objective-C and
TypeScript without asking Lean to be anything but Lean. This page explains how the pieces fit
together and why they are arranged this way.

## Lean compiles Lean

The obvious way to translate Lean into Rust would be to read Lean source, or Lean's
elaborated terms, and translate them. lungo deliberately does neither. Lean source is
extended by macros, notations and elaborators that only Lean can run, and elaborated terms
still contain proofs, types, type classes and definitions by well-founded recursion, whose
executable meaning Lean's compiler decides. A translator working at that level would have to
reimplement a large part of Lean, and would sooner or later disagree with it.

Instead, lungo lets Lean do everything up to the point where Lean's own compiler hands
its output to a backend. Lake builds the project with the exact toolchain it pins, so the
project's syntax extensions run, its proofs are checked by Lean's kernel, and its definitions
are compiled by Lean's compiler, exactly as for a native Lean build. lungo starts from the
compiled program: a small, first-order, imperative intermediate language with explicit
reference counting, which is what a backend has to implement anyway. Everything above it
stays Lean's responsibility.

This is also why a Lean project needs no annotations or changes to be used from Rust, and why
invalid Lean or a false proof fails the Rust build: they fail the Lean build first.

## The pipeline

```text
build.rs / lungo ──► lake build ──► worker ──► Bridge IR + interface ──► verifier ──► generators
                     (Lean)         (Lean)                                (Rust)       (Rust)
```

1. **Lake** elaborates, checks and compiles the root modules. lungo only reads what Lake
   produces; it never parses Lean source.
2. **The worker** is a Lean program, compiled against the project's toolchain, that loads the
   compiled environment, selects the program (the root modules' code, their initializers,
   everything they call) and encodes it as **Bridge IR** together with metadata: exported
   declarations and their types, extern symbols, source locations and trust information. It
   runs as a separate process, so the build survives it crashing or hanging.
3. **The verifier** checks the Bridge IR independently of the worker that produced it.
4. **The generators** write the program and its interface in each requested language (see
   [below](#one-program-many-languages)).
5. **Publication** replaces each output directory atomically, and in `build.rs` tells Cargo
   which files to watch.

The same library runs from `build.rs` and from the [`lungo` command](../reference/lungo-cli.md),
so the command line always sees what the build sees.

## One program, many languages

Generating code for a language has two parts, which lungo keeps apart:

- **Execution**: the compiled program, instruction by instruction, with Lean's reference
  counting and object layout. There are two: Rust (the compiler layer of a Rust crate) and
  C (compiled by the host language's C toolchain: cgo, Xcode, a Python build, clang for
  WebAssembly). Both run on the same runtime, lungo's Rust port of Lean's runtime; C calls
  it through its C ABI (`lungo.h`), as Lean's own C backend calls `libleanrt`.
- **Projection**: the API of the exported declarations in the language's own terms: Rust
  structs and enums, Go structs and sealed interfaces, Python dataclasses, Swift structs and
  `indirect enum`s, TypeScript objects and tagged unions, C values with typed accessors.

The description every projection starts from is the program's *interface*: the exported
functions with their types, the types they use, the externs the host implements, and the
entry point. It is to lungo what the `FileDescriptorSet` is to `protoc`: a generator is a
function from it to files, whether it is built in or a [plugin](../reference/plugins.md)
(`lungo-gen-<language>`) receiving it as JSON.

Rust keeps its own projection, which converts Lean objects directly. Every other language
crosses a single boundary: the program's C entry points take their arguments and return
their results in one binary encoding, the [wire format](../reference/wire-format.md), and
each language's support library encodes and decodes it. One call is one crossing, however
deep the value; recursive types, arbitrary-precision numbers and polymorphic functions are
handled by the one codec in the runtime, driven by a table of the program's types. Values a
language cannot hold as data (Lean closures, opaque values, `IO.Error`s) cross as handles,
and functions of the host language cross into Lean as callbacks.

A process has one runtime: the generated packages of any number of programs share it, and
it is prebuilt for every platform and distributed with each release, so that no one using
Go, Python, Swift or TypeScript compiles Rust. See [the runtime distribution](distribution.md).

## Two layers of generated code

The compiler layer is mechanical and not meant to be read or called: it mirrors Lean's
compiled program, including its reference counting and in-place updates, with Lean's
runtime object layout. The facade is the part designed for people: Lean structures become
Rust structs, `Nat` becomes an unbounded `Nat`, `IO` becomes `Result`, and conversions happen
at the boundary. Keeping them apart lets the compiler layer stay faithful to Lean while the
facade stays idiomatic Rust.

## The Lake project is the configuration

A Lake project already says what it is made of: its package name and its default targets,
the libraries and executables `lake build` builds. lungo reads both instead of asking for
them again. By default it compiles the root modules of the default targets, as Lake itself
resolves them (so a `lakefile.lean` computing its targets is understood as well as a
`lakefile.toml`), exports what those modules define, and names the generated module after
the package. A `build.rs` therefore needs only the project's location, and a crate includes
the result by the package's name with `lungo::include_lean!`, much as a crate generating
gRPC code with `tonic` compiles `.proto` files and includes them by package with
`include_proto!`.

What Lean cannot know, the application configures: which Rust traits generated types
derive, attributes for libraries such as `serde`, and which Lean types are provided by Rust
types that already exist, such as those another lungo build generated. These settings name
Lean declarations, and one that names nothing is an error rather than a silent no-op, so
configuration cannot drift away from the Lean code it shapes.

## Why the toolchain is pinned exactly

Lean's compiler output is not a stable format; it changes between releases. The worker is
therefore built for one toolchain, and its adapter to the compiler's data structures is
validated for that release. A project pinning an unsupported toolchain is rejected before
anything is compiled, rather than translated by an adapter that might misread it. Supporting
a new Lean release means adding an adapter for it.

## Builds are reproducible and offline

A Cargo build must not change the project, download anything, or depend on where it runs.
lungo never installs toolchains or fetches Lake dependencies during a build (the manifest
must be committed and dependencies present), never writes into the Lean project, and writes
output that contains no machine-specific paths. Its output depends only on the inputs recorded
in its build key, so an unchanged project reuses the previous output, and two builds of the
same project produce the same files.

## Further reading

- [The compiler boundary](compiler-boundary.md) — what exactly lungo takes from Lean
- [The runtime](runtime.md) — how Lean's object model is reproduced in Rust
- [Trust and verification](trust-and-verification.md) — what one has to trust, and how it is
  checked
