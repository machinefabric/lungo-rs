---
title: "Generated packages"
description: "What lungo generate writes for C, Go, Python, Swift and TypeScript: package layouts, how Lean types map to each language, names, errors and threads."
---

`lungo generate --<language>_out=DIR` writes a package that holds the program (as C, or for
TypeScript as WebAssembly) and its API in the language. It builds with the language's own
tools and runs on the lungo runtime, which it takes from the lungo release (or a
[local distribution](lungo-cli.md#the-runtime)) at exactly the version that generated it. The
Rust output is described in [generated code](generated-code.md) and
[Lean types in Rust](type-mapping.md).

Below, *the program's id* is its name as a C identifier (`polyglot` for a Lake package
`polyglot`).

## Names

An exported declaration is named after the shortest suffix of its Lean name that no other
declaration of its kind ends with: `Geometry.Shape.area` is `area`, unless another function
also ends with `area`, in which case both keep one more component (`shape_area`,
`other_area`). The suffix is converted to the language's conventions (`snake_case` in C and
Python, `UpperCamelCase` for Go and all types, `lowerCamelCase` for Swift and TypeScript
members) and escaped where it is a keyword. Two declarations that would still get the same
name (`foo` and `Foo`) are an error ([`LNG0107`](errors.md#lng0107)) naming both: rename one
in Lean.

## Lean types

| Lean | C | Go | Python | Swift | TypeScript |
| --- | --- | --- | --- | --- | --- |
| `Nat` | `lungo_value` | `*big.Int` | `int` | `LungoNat` | `bigint` |
| `Int` | `lungo_value` | `*big.Int` | `int` | `LungoInt` | `bigint` |
| `Bool` | `lungo_value` | `bool` | `bool` | `Bool` | `boolean` |
| `UInt8`–`UInt32`, `Int8`–`Int32` | `lungo_value` | `uint8`–`int32` | `int` | `UInt8`–`Int32` | `number` |
| `UInt64`, `USize`, `Int64`, `ISize` | `lungo_value` | `uint64`, `int64` | `int` | `UInt64`, `Int64` | `bigint` |
| `Float`, `Float32` | `lungo_value` | `float64`, `float32` | `float` | `Double`, `Float` | `number` |
| `Char` | `lungo_value` | `rune` | `str` of length 1 | `Unicode.Scalar` | `string` of one code point |
| `String` | `lungo_value` | `string` | `str` | `String` | `string` |
| `Unit` | `lungo_value` | `lungo.Unit` | `None` | `LungoUnit` | `null` |
| `ByteArray` | `lungo_value` | `[]byte` | `bytes` | `[UInt8]` | `Uint8Array` |
| `FloatArray` | `lungo_value` | `[]float64` | `list[float]` | `[Double]` | `number[]` |
| `Option α` | `lungo_value` | `lungo.Option[A]` | `A` or `None` | `A?` | `A` or `null` |
| `List α`, `Array α` | `lungo_value` | `[]A` | `list[A]` | `[A]` | `A[]` |
| `α × β` | `lungo_value` | `lungo.Pair[A, B]` | `tuple[A, B]` | `(A, B)` | `[A, B]` |
| `Except ε α` | `lungo_value` | `lungo.Except[E, A]` | `lungo_py.Ok` / `lungo_py.Err` | `LungoExcept<E, A>` | `{ ok, value }` / `{ ok, error }` |
| `α → β` | `lungo_value` | `func(A) (B, error)` | callable | `(A) throws -> B` | function |
| a structure | `lungo_value` | struct | frozen dataclass | struct | object |
| an inductive type | `lungo_value` | sealed interface, a struct per constructor | base class, a frozen dataclass per constructor | `indirect enum` | objects tagged with `kind` |
| a polymorphic type | `lungo_type` argument | generic type | `Generic` class | generic type | generic type |
| an [opaque type](#opaque-types) | `lungo_value` | struct around `lungo.Opaque` | subclass of `lungo_py.Opaque` | struct around `LungoOpaque` | subclass of `Opaque` |
| any other type | `lungo_value` | `lungo.Opaque` | `lungo_py.Opaque` | `LungoOpaque` | `Opaque` |

Numbers are checked when they cross into Lean: a negative `Nat`, a Go `int32` out of a
`UInt16`'s range, a string that is not valid Unicode, a nil value of an inductive type are
rejected (see [errors](#errors)), never truncated. An `Option` whose values can themselves be
`None`/`null` (`Option (Option α)`, `Option Unit`) is `Some(value)` (Python,
TypeScript) where it is `some`. A structure that contains itself by value (through options,
pairs or other structures) is a pointer in Go and a final class in Swift. Values Lean cannot
give a language type (IO actions stored as data, a `Type`, a value of a type the program does
not describe) are opaque: handles that keep the Lean value alive until they are closed or
collected.

### Opaque types

An inductive type whose values Lean code alone can make — one with a proof among its fields,
say, like a structure of a value and the proof that it is well formed — is *opaque*: its values
cross as handles, which the program's functions make and read. It is still a type of its own:
`Wf` in Go is `struct{ lungo.Opaque }`, a subclass of `lungo_py.Opaque` in Python, a struct in
Swift, a subclass of `Opaque` in TypeScript, so one opaque type's value cannot be passed where
another's is expected. Type parameters of an opaque type are not part of its binding type.
Types that are not inductive, or have indices, are plain opaque values.

### Layout fingerprints

Every named type records its layout fingerprint: `<Name>Fingerprint` (Go),
`<Class>.__lungo_fingerprint__` (Python), `<Type>.lungoFingerprint` (Swift),
`leanTypes["<Lean name>"].fingerprint` (TypeScript), `<ID>_<TYPE>_FINGERPRINT` (C), and
`lungo::LeanLayout::FINGERPRINT` in Rust. A package using another's type as an
[extern type](configuration.md#extern-types) checks it before it runs.

A polymorphic function takes a type descriptor for each type parameter, first:
`Size(lungo.NatType, tree)` (Go), `size(lungo_py.NAT, tree)` (Python), `size(Lungo.nat,
tree)` (Swift), `p.size(L.NAT, tree)` (TypeScript), `polyglot_size(nat_type, tree, &result,
&error)` (C). Each
named type has a descriptor function (`TreeType(a)`, `tree_type(a)`, `Tree.lungoType(a)`,
`treeType(a)`, `polyglot_tree_type(a)`).

## Errors

| Lean result | C (status) | Go | Python | Swift | TypeScript |
| --- | --- | --- | --- | --- | --- |
| a value | `LUNGO_OK` | `(T, nil)` | the value | the value | the value |
| `IO α` failing | `LUNGO_FAILED`, `LUNGO_ERROR_IO` | `*lungo.IOError` | `lungo_py.LeanIOError` | `LungoIOError` | `LeanIOError` |
| `EIO ε α` failing | `LUNGO_FAILED`, `LUNGO_ERROR_VALUE` | `*lungo.Error[E]` | `lungo_py.LeanError` | `LungoError<E>` | `LeanError` |
| arguments Lean cannot represent | `LUNGO_MALFORMED` | `*lungo.MalformedError` | `lungo_py.MalformedError` | `LungoMalformed` | `MalformedError` |

A function whose result is `Unit` returns nothing (only its error) in Go, Swift and
TypeScript.

## Host externs

An extern listed in `host-externs` is implemented by the application (see
[how to call host code from Lean](../how-to/call-host-code-from-lean.md)): a Go `Host`
interface installed with `SetHost`, a Python `Host` protocol installed with `set_host`, a
Swift `<Module>Host` protocol installed with `setHost`, a TypeScript `<Program>Host` object
given to `load`, C functions registered with `<id>_implement_<name>`. They must be installed
before the program's first call; a program used without them fails, naming the missing
extern.

## Threads

The program may be called from any number of threads at once (Go goroutines, Python threads,
Swift tasks and queues, C threads); calls release the Python GIL. Host functions may be
called on any thread, including threads Lean's tasks run on. WebAssembly has one thread:
Lean's tasks run when they are spawned, and the program cannot use child processes, sockets,
signals or timers ([`LNG0408`](errors.md#lng0408)).

One process has one lungo runtime and one host: the support library of its language. The C
value API's host functions and another language's support library cannot be used in the
same process.

## C

```text
CMakeLists.txt          the library <id> (target <id>::<id>) and the runtime
include/<id>.h          the API
src/<id>.c
program/                the program: lungo.h, <id>_program.h, <id>_program.c,
                        <id>_boundary.c, modules/*.c
```

The CMake project downloads the runtime archive of the lungo release for the target, checks
its SHA-256 digest (`URL_HASH`), and links its static library (`lungo::runtime`). With
`-D<ID>_FETCH_LUNGO=OFF` it uses an installed runtime instead (`find_package(lungo <version>
EXACT)`; `CMAKE_PREFIX_PATH` or `lungo_DIR` locates it); `-D<ID>_LUNGO_TARGET=<triple>`
chooses the archive when the target cannot be inferred. Include it with `add_subdirectory`.

Every value is a `lungo_value *` of the [C API](c-api.md). The API adds, per type, its
descriptor (`<id>_<type>_type(...)`), a constructor function per constructor
(`<id>_<type>_<ctor>(fields…)`, taking ownership of the fields), a macro of each constructor's
index (`<ID>_<TYPE>_<CTOR>`) and an accessor per field (`<id>_<type>_<field>(v)`, or
`<id>_<type>_<ctor>_<field>(v)` for types of several constructors); per function
`int32_t <id>_<function>(type arguments…, arguments…, lungo_value **result, lungo_error
**error)`; `<id>_implement_<extern>(f, ctx, drop)`; `<id>_initialize()` and
`<id>_run_main(argc, argv)`. Objective-C uses the same API (see Swift).

## Go

```text
<id>.go                 the package (cgo)
*.c, *.h                the program; module files are module_<stem>_lean.c
```

The package imports `github.com/machinefabric/lungo-go` and requires exactly the module
version of the lungo release that generated it (`const _ = lungo.EnforceVersion<version>`
does not compile with another). The module carries the runtime for linux/amd64,
linux/arm64, darwin/amd64, darwin/arm64 and windows/amd64 and links it once per binary. Build
with cgo enabled and a C compiler (on Windows, MinGW-w64 GCC). The package name is the
program's id (option `package`).

Functions return `(T, error)`; types are Go types with a descriptor function each
(`PointType()`, `TreeType[A](typeA)`); `SetHost(h Host)` installs the host externs;
`RunMain(args) (int, error)` runs `main`.

## Python

```text
pyproject.toml          scikit-build-core; requires lungo-py
CMakeLists.txt          builds the program as the shared library of the package
program/
src/<package>/__init__.py, py.typed
```

`pip install DIR` builds a wheel (`py3-none-<platform>`: the program is a shared library
loaded with `ctypes`, not a CPython extension, so one wheel serves every Python 3). It
depends on exactly the `lungo-py` of the release, which carries the runtime. Functions are
module functions; types are frozen dataclasses; `set_host(host)` installs the host externs;
`run_main(args)` runs `main`.

## Swift

```text
Package.swift           depends on lungo-swift at exactly the release's version
Sources/<Module>Program/  the program and the C API (include/<id>.h: for C and Objective-C)
Sources/<Module>/<Module>.swift
```

The package has two products: `<Module>` (the Swift API) and `<Module>Program` (the C API,
which Objective-C imports with `@import <Module>Program;`). `lungo-swift` provides `LungoKit`
and the runtime as an XCFramework (macOS 12 and later, iOS 15 and later, the iOS simulator).
Functions are module functions that `throw`; types are structs and `indirect enum`s;
`setHost(_:)` installs the `<Module>Host`; `runMain(_:)` runs `main`.

## TypeScript

```text
package.json            depends on lungo-ts at exactly the release's version
index.js, index.d.ts    ES module and its declarations
program.wasm            the program and the runtime, for wasm32-wasip1
```

`lungo generate` links `program.wasm` with the pinned wasi-sdk. `await load(options)`
instantiates it (on Node.js 20 and later with `node:wasi`; in browsers with lungo-ts's
`BrowserWasi`, or any WASI given as `options.wasi`) and returns an object whose methods are
the program's functions; `options.host` implements the host externs. Programs using
primitives WebAssembly lacks cannot be generated ([`LNG0408`](errors.md#lng0408)).

## Embedded packages

A library that already is a package in its language — a Python distribution, an npm package,
a Swift package, a CMake project — embeds the program in it with the generator option `embed`
instead of depending on a package of the program's own. lungo owns the output directory and
replaces it as a whole, so it is a directory of the host's, never its root. The host's
manifest provides what the generated one would have:

| Language | The output directory | The host provides |
| --- | --- | --- |
| C (`embed=true`) | `include/`, `src/`, `program/`, and a `CMakeLists.txt` defining the library `<id>` | the target `lungo::runtime` (`find_package(lungo <version> EXACT CONFIG)`) before `add_subdirectory`; linking the packages providing extern types to `<id>` |
| Python (`embed=<module>`) | the module: `__init__.py`, `program/`, and a `CMakeLists.txt` building the program and installing it into the module | a scikit-build-core build whose `CMakeLists.txt` adds the directory with `add_subdirectory`; `lungo-py==<version>` in `build-system.requires` and `dependencies` |
| Swift (`embed=true`) | `<Module>Program/` and `<Module>/`, the two targets | the targets, by `path`: `<Module>Program` depending on `LungoKit` with `cSettings: [.headerSearchPath("program")]`, and `<Module>` depending on it and `LungoKit`; `lungo-swift` at exactly the release's version |
| TypeScript (`embed=true`) | `index.js`, `index.d.ts`, `program.wasm` | `lungo-ts` at exactly the release's version, and `program.wasm` in the published files |
