# lean2rust

lean2rust compiles Lean through Lean's own frontend and compiler and captures the final impure LCNF as a versioned Bridge IR. The architecture is specified in [docs/DESIGN.md](docs/DESIGN.md).

The current repository contains the Lean 4.34.1 worker, a typed Rust BIR decoder, and compiler-boundary fixtures. It does not yet contain a Rust code generator or target runtime, so it cannot produce a Rust crate from Lean source.

## Worker

Build the worker with its pinned Lean toolchain:

```sh
cd worker
lake build
```

The worker reads a module through the normal Lean frontend. It uses the calling Lake project's `LEAN_SRC_PATH` and `LEAN_PATH`; imported dependencies must already be built. It writes a length-prefixed Bridge IR frame at the requested output path:

```sh
cd compiler-tests/fixtures/simple
lake env sh -c 'LEAN_PATH="../../../worker/.lake/build/lib/lean:$LEAN_PATH" ../../../worker/.lake/build/bin/lean2rust-worker Simple /tmp/simple.bir'
```

The frame starts with `L2RB`, followed by a 32-bit little-endian payload length and a UTF-8 JSON payload. The payload identifies the protocol and BIR versions and contains compiler declarations, including extern declarations and generated auxiliaries. Lean diagnostics are written to stderr, and a failed run leaves no output frame.

The `simple` fixture uses Lean's `module` header; `legacy` covers the older header style. The worker does not require annotations or changes to either source file.

Run the compiler-boundary integration tests from the repository root:

```sh
python3 -m unittest compiler-tests/test_worker.py -v
cargo test --workspace --offline
```
