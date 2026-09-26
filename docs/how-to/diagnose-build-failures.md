# How to diagnose a failing patina build

This guide shows how to get from a failing `cargo build` to the cause. Every patina error
starts with a code, `error[PTNxxxx]`; the [error reference](../reference/errors.md) describes
each one.

## Find the message

Cargo prints the build script's error under `--- stderr`:

```text
error: failed to run custom build command for `shapes v0.1.0`
  ...
  --- stderr
  Error: error[PTN0201]: Lean elaboration failed
```

Read the code, then follow the section for its range.

## Lean errors (`PTN0201`)

The message contains Lean's diagnostics with file, line and column. Fix the Lean code; to
iterate faster, build the Lean project directly:

```sh
cd lean && lake build
```

## Project and toolchain errors (`PTN01xx`)

- **The toolchain is not installed** ([`PTN0103`](../reference/errors.md#ptn0103)): run the
  command the message gives, `elan toolchain install <toolchain>`.
- **The toolchain is not supported** ([`PTN0102`](../reference/errors.md#ptn0102)): pin a
  supported toolchain in `lean-toolchain` (see [platforms](../reference/platforms.md)).
- **The manifest is missing** or a dependency is not materialized
  ([`PTN0101`](../reference/errors.md#ptn0101)): run `lake update` in the Lean project once,
  and commit `lake-manifest.json`. For an existing manifest, `cargo patina setup`
  materializes its dependencies without changing it.
- **The configuration cannot take effect** ([`PTN0107`](../reference/errors.md#ptn0107)): the
  message names the setting. A shaping path that selects nothing is usually misspelled;
  `cargo patina mappings` lists the Lean names of every generated type, function and field.

## Extern errors (`PTN04xx`)

For [`PTN0401`](../reference/errors.md#ptn0401), the message names the declaration, symbol,
Lean type and source. If the declaration is yours, implement and map it
([How to call Rust functions from Lean](call-rust-from-lean.md)). If it belongs to a
dependency, the dependency relies on native code that patina does not provide; implement the
symbol in Rust with `rust_extern`, or avoid the declaration.

`cargo patina externs` lists how every extern of the program is implemented.

## Worker errors (`PTN03xx`)

- **Timeouts and resource limits** ([`PTN0304`](../reference/errors.md#ptn0304),
  [`PTN0305`](../reference/errors.md#ptn0305)): raise the limit, or find the Lean code that
  runs long during elaboration (`lake build` shows it too).
- **Crashes** ([`PTN0303`](../reference/errors.md#ptn0303)): the message contains the
  worker's output. Run `cargo patina prepare`, which verifies the cached worker and
  rebuilds it if it is damaged; if the crash persists, report it with that output.
- **Unsatisfiable requests** ([`PTN0301`](../reference/errors.md#ptn0301)): check the names
  in `root_module`, `export` and `export_module` against the Lean project. A package without
  `defaultTargets` needs `root_module`.

## Errors that indicate a patina defect

[`PTN0302`](../reference/errors.md#ptn0302), [`PTN0404`](../reference/errors.md#ptn0404) and
[`PTN05xx`](../reference/errors.md#code-generation) mean patina could not handle valid
compiler output. Report them with the message, the Lean toolchain, and if possible
`cargo patina ir <declaration>` for the declaration named.
