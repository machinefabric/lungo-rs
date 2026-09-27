---
title: "How to distribute generated packages"
description: "Publish a package lungo generated as a Go module, a Python wheel, a Swift package, an npm package or a CMake library."
---

A generated package depends on the lungo runtime of the release that generated it, which the
language's support library (or, for C, the CMake project) downloads and verifies. It can be
published like any package of its language. Generate it with a release of `lungo` (not a
[local distribution](../reference/lungo-cli.md#the-runtime), whose packages build only on the
machine that made them), and regenerate it when the Lean project or lungo changes; the
output directory holds `build-info.json`, which records what it was generated from.

## Go

Generate into a directory of your module and commit it:

```sh
lungo generate --go_out=internal/geometry --go_opt=package=geometry
go mod tidy   # adds github.com/machinefabric/lungo-go at the release's version
```

Users of your module need cgo and a C compiler; nothing else. Keep `lungo-go` at the version
the package was generated with: the package does not compile against another.

## Python

```sh
lungo generate --python_out=geometry-py --python_opt=distribution=geometry,version=1.2.0
python -m build --wheel geometry-py
```

The wheel is `py3-none-<platform>` (one per platform, for every Python 3): build it on each
platform you publish for, for example with `cibuildwheel`. On Linux, repair it with
`auditwheel repair --exclude liblungo.so`: the runtime comes from `lungo-py`, which the wheel
depends on. On macOS, `delocate` likewise must exclude `liblungo.dylib`. Publish the wheels as
usual (`twine upload`, or trusted publishing).

## Swift

Generate into its own repository (or a directory of one) and tag it:

```sh
lungo generate --swift_out=GeometryKit --swift_opt=module=Geometry
```

Applications add it with `.package(url: …, exact: …)` and use the `Geometry` product;
Objective-C code uses the `GeometryProgram` product. Its dependency on `lungo-swift` brings
the runtime as an XCFramework for macOS, iOS and the iOS simulator.

## npm

```sh
lungo generate --ts_out=geometry-js --ts_opt=package=@acme/geometry,version=1.2.0
cd geometry-js && npm publish --access public
```

The package holds `program.wasm` and runs on Node.js and in browsers (bundlers must copy
`program.wasm` next to `index.js`, or the application passes the module to
`load({ module })`).

## C and C++

Ship the generated directory (for example as a Git submodule or a source archive) and
include it with `add_subdirectory`; its CMake project downloads and verifies the runtime.
Offline builds install the runtime (`lungo runtime fetch`, or the release archive) and
configure with `-D<ID>_FETCH_LUNGO=OFF -DCMAKE_PREFIX_PATH=<runtime>`.
