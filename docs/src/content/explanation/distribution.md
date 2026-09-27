---
title: "The runtime distribution"
description: "Why every language runs on one prebuilt runtime, how it is distributed, and what a user of a generated package has to trust."
---

Every generated package runs its program on the lungo runtime, the Rust port of Lean's
runtime that Rust crates build from source. For the other languages it is prebuilt: someone
using a Lean program from Go, Python, Swift or TypeScript should need their language's
toolchain and a C compiler, not Rust. This page explains how the runtime reaches them and why
it is arranged this way.

## One runtime, many packages

Lean's own backend generates C and links it against Lean's runtime library; lungo's C
backend does the same, against `liblungo`. The runtime is the part of a program that does
not depend on the program (objects, numbers, strings, arrays, tasks, `IO`), so it is built
once per platform, released, and shared: by every generated package in a process, and by
every program version generated with the same lungo release.

Sharing is also a requirement. The runtime has process-wide state: the task manager, the
handles the host holds, the host's entry points, the standard streams. Two copies in one
process would split it. Each language's support library therefore links the runtime once
(the Go module links it into the binary, `lungo-py` loads its shared library into the
global symbol namespace, the Swift package links the XCFramework, the C package links the
static library), and generated packages only reference its symbols.

## What a release contains

Each lungo release publishes, with the same version:

- the runtime for every [platform](../reference/platforms.md), as archives with the header,
  static and shared libraries, a CMake package and a pkg-config file, and for Apple
  platforms as an XCFramework;
- the `lungo` command, which embeds the release's runtime manifest (every archive's URL and
  SHA-256 digest);
- the support libraries: `lungo-py` on PyPI (with the runtime for each platform in its
  wheels), `lungo-ts` on npm, the Go module `github.com/machinefabric/lungo-go` (with the
  runtime for each platform), and the Swift package `machinefabric/lungo-swift` (whose
  `LungoRuntime` target is the XCFramework, by URL and checksum);
- the Rust crates.

A generated package refers to exactly this release: its version is part of what generated
it, because the generated C calls the runtime's internal interface. A mismatch is detected
where it can be: the C ABI version fails at link time, the Go module's version marker at
compile time, the Python and TypeScript packages when they load, and the CMake package's
version is required exactly.

## What has to be trusted

Everything a user downloads is checked against digests that come from the release itself:

- The runtime archives are listed with SHA-256 digests in the manifest embedded in the
  `lungo` command, and a generated C package's CMake checks its archive (`URL_HASH`);
  `lungo runtime fetch` refuses a mismatch ([`LNG0109`](../reference/errors.md#lng0109)).
- The install scripts check the `lungo` command against the release's `SHA256SUMS`.
- The release's artifacts carry build provenance attestations (GitHub artifact
  attestations), verifiable with `gh attestation verify`.
- PyPI and npm packages are published with trusted publishing, from the release workflow of
  the repository; the Go and Swift distribution repositories are written only by it.

The wasi-sdk that links WebAssembly programs is pinned the same way: `lungo` downloads a
fixed release and checks its SHA-256 digest before using it.

What remains to be trusted is the lungo repository and its release workflow, as for any
compiler toolchain, and Lean itself, whose compiler produced the program.

## Developing lungo

Before a release exists, there is nothing to download. A *local distribution* is the same
layout built from the repository (`cargo run -p lungo-dist -- local --out DIR`), and
`lungo generate --runtime-dir DIR` makes the generated packages refer to it. Such packages
work only on that machine; they are how lungo's own end-to-end tests exercise every language
before a release.
