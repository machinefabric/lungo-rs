---
title: "How to diagnose a failing lungo build"
description: "Get from a failing cargo build to its cause, by error code."
---

This guide shows how to get from a failing `cargo build` to the cause. Every lungo error
starts with a code, `error[LNGxxxx]`; the [error reference](../reference/errors.md) describes
each one.

## Find the message

Cargo prints the build script's error under `--- stderr`:

```text
error: failed to run custom build command for `shapes v0.1.0`
  ...
  --- stderr
  Error: error[LNG0201]: Lean elaboration failed
```

Read the code, then follow the section for its range.

## Lean errors (`LNG0201`)

The message contains Lean's diagnostics with file, line and column. Fix the Lean code; to
iterate faster, build the Lean project directly:

```sh
cd lean && lake build
```

## Project and toolchain errors (`LNG01xx`)

- **The toolchain is not installed** ([`LNG0103`](../reference/errors.md#lng0103)): run the
  command the message gives, `elan toolchain install <toolchain>`.
- **The toolchain is not supported** ([`LNG0102`](../reference/errors.md#lng0102)): pin a
  supported toolchain in `lean-toolchain` (see [platforms](../reference/platforms.md)).
- **The manifest is missing** or a dependency is not materialized
  ([`LNG0101`](../reference/errors.md#lng0101)): run `lake update` in the Lean project once,
  and commit `lake-manifest.json`. For an existing manifest, `lungo setup`
  materializes its dependencies without changing it.
- **The configuration cannot take effect** ([`LNG0107`](../reference/errors.md#lng0107)): the
  message names the setting. A shaping path that selects nothing is usually misspelled;
  `lungo mappings` lists the Lean names of every generated type, function and field.

## Extern errors (`LNG04xx`)

For [`LNG0401`](../reference/errors.md#lng0401), the message names the declaration, symbol,
Lean type and source. If the declaration is yours, implement and map it
([How to call Rust functions from Lean](call-rust-from-lean.md)). If it belongs to a
dependency, the dependency relies on native code that lungo does not provide; implement the
symbol in Rust with `rust_extern`, or avoid the declaration.

`lungo externs` lists how every extern of the program is implemented.

## Worker errors (`LNG03xx`)

- **Timeouts and resource limits** ([`LNG0304`](../reference/errors.md#lng0304),
  [`LNG0305`](../reference/errors.md#lng0305)): raise the limit, or find the Lean code that
  runs long during elaboration (`lake build` shows it too).
- **Crashes** ([`LNG0303`](../reference/errors.md#lng0303)): the message contains the
  worker's output. Run `lungo prepare`, which verifies the cached worker and
  rebuilds it if it is damaged; if the crash persists, report it with that output.
- **Unsatisfiable requests** ([`LNG0301`](../reference/errors.md#lng0301)): check the names
  in `root_module`, `export` and `export_module` against the Lean project. A package without
  `defaultTargets` needs `root_module`.

## Errors that indicate a lungo defect

[`LNG0302`](../reference/errors.md#lng0302), [`LNG0404`](../reference/errors.md#lng0404) and
[`LNG05xx`](../reference/errors.md#code-generation) mean lungo could not handle valid
compiler output. Report them with the message, the Lean toolchain, and if possible
`lungo ir <declaration>` for the declaration named.
