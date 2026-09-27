---
title: "Errors"
description: "Every lungo error code, what causes it, and what to do about it."
---

Every error lungo reports has a stable code, printed as `error[LNGxxxx]: …`. In Rust,
`lungo_build::Error::code()` returns it as a `lungo_build::ErrorCode`. A code is
never reused for a different condition.

| Range | Stage |
| --- | --- |
| `LNG01xx` | Project and toolchain |
| `LNG02xx` | Lean |
| `LNG03xx` | Worker |
| `LNG04xx` | Externs |
| `LNG05xx` | Code generation |
| `LNG06xx` | Trust policy |

For fixing common failures, see
[How to diagnose a failing lungo build](../how-to/diagnose-build-failures.md).

## Project and toolchain

### LNG0101

**Invalid Lean project.** The Lake project is incomplete or inconsistent: no
`lakefile.toml`/`lakefile.lean`, a missing or invalid `lake-manifest.json`, a locked
dependency that is not materialized, a malformed `lean-toolchain`, or a Lake configuration
modified during the build.

```text
error[LNG0101]: invalid Lean project: /work/app/lean/lake-manifest.json is missing; run `lake update` once and commit the manifest
```

### LNG0102

**Unsupported Lean toolchain.** `lean-toolchain` names a toolchain lungo does not
support, including floating names such as `stable`. The message lists the supported
toolchains; see [platforms](platforms.md).

### LNG0103

**Lean toolchain not installed.** The pinned toolchain is not installed, and the
configuration does not permit installing it. Install it with
`elan toolchain install <toolchain>` or `lungo setup`.

### LNG0104

**Unsupported build environment.** The environment lacks something lungo requires (a
Cargo variable outside a build script, a home or cache directory), or cannot provide a
requested capability: `LeanOracle` mode when cross-compiling or on an MSVC target, a worker
memory limit where it cannot be enforced, a toolchain whose `lean --version` does not match
its pin.

### LNG0105

**Input/output error.** A file-system operation failed. The message names the operation and
the operating-system error.

### LNG0106

**External command failed.** A tool lungo runs failed: `lake env`, the worker's own build,
or in `LeanOracle` mode Lake's C generation, `leanc` or `llvm-ar`. The message contains the
command's output.

### LNG0107

**Invalid configuration.** The build configuration is malformed or cannot take effect: an
unknown key or wrong value in `lungo.toml`, a generated-module name that is not letters,
digits, `_` and `-` (including a Lake package name that cannot be used without
[`name`](configuration.md#output)), two Lean projects in one build script that would generate
the same module, no `OUT_DIR` and no `out_dir` outside a build script, or a
[shaping](configuration.md#shaping-the-generated-code) setting (`type_attribute`,
`struct_attribute`, `enum_attribute`, `field_attribute`, `skip_debug`, `disable_comments`,
`extern_type`) whose path selects nothing.

```text
error[LNG0107]: field_attribute path `Geometry.Point.z` selects no field of a generated type
```

### LNG0108

**lungo runtime unavailable.** A language binding needs the prebuilt lungo runtime, and none
is available: this `lungo` is a development build, which knows no release, and no local
runtime distribution was given with `--runtime-dir`; or the release has no runtime for the
requested target. See [the runtime](lungo-cli.md#runtime).

### LNG0109

**Runtime artifact checksum mismatch.** A runtime artifact downloaded by `lungo runtime fetch`
(or found in the cache) does not have the SHA-256 digest recorded in this `lungo` release.
The artifact is not used; the cached copy is removed. A persistent mismatch means the
download was tampered with or corrupted in transit.

## Lean

### LNG0201

**Lean rejected the program.** Elaboration, kernel checking of proofs, or Lean's compiler
failed. The message contains Lean's diagnostics with their source positions.

```text
error[LNG0201]: Lean elaboration failed

error: Shapes.lean:12:75: unsolved goals
```

## Worker

### LNG0301

**Request cannot be satisfied.** The configuration asks for something the program does not
provide, for example an export that does not exist, or a root module outside the project's
root package.

### LNG0302

**Compiler output not representable.** The worker's adapter for the toolchain cannot encode
Lean's compiler output as Bridge IR. This indicates a gap in lungo's support for the
toolchain.

### LNG0303

**Worker crashed.** The worker process ended without a response: it crashed, exited, or was
killed. The message contains its exit status, its output, and any resource limits in effect.

### LNG0304

**Worker timed out.** The worker did not finish within `worker_timeout`; it and every process
it started were killed.

### LNG0305

**Worker resource limit exceeded.** The operating system stopped the worker for exceeding
`worker_cpu_limit`.

### LNG0306

**Worker protocol error.** The worker's response is malformed, or its protocol, Bridge IR,
or adapter version, or its Lean commit, differs from what lungo expects. A stale cached
worker is rebuilt automatically; persistent protocol errors indicate an installation problem.

## Externs

### LNG0401

**Unresolved extern symbol.** An `@[extern]` declaration reachable from the root modules is
implemented neither by a Lean `@[export]` definition, nor by the lungo runtime, nor by a
`rust_extern` mapping. The message names the declaration, its symbol, Lean type, required
representation, and source location.

```text
error[LNG0401]: unresolved Lean external symbol

Declaration: Unknown.providerSend
Symbol: provider_send
Lean type: Nat → Nat
Expected runtime representation: (tobj) -> tobj
Source: lean/Unknown.lean:3:1

Provide a Rust mapping with Builder::rust_extern("provider_send", "crate::path::to::function")
```

### LNG0402

**Extern symbol implemented twice.** A symbol implemented by a Lean `@[export]` definition is
also provided by the runtime or a `rust_extern` mapping.

### LNG0403

**Runtime symbol remapped.** A `rust_extern` mapping names a symbol the lungo runtime
implements; runtime primitives cannot be replaced.

### LNG0404

**Extern representation mismatch.** A runtime primitive's parameter or result
representations differ from those of the Lean declaration using its symbol. This indicates a
lungo defect.

### LNG0405

**Unused `rust_extern` mapping.** A mapping names a symbol that no extern declaration
reachable from the root modules uses.

### LNG0406

**Extern has no Rust signature.** A `rust_extern` mapping targets an extern whose Lean type
does not determine a Rust function signature (for example, one without a source-level
constant). Such externs can only be implemented in Lean or by the runtime.

### LNG0407

**Extern form unsupported in `LeanOracle` mode.** In `LeanOracle` mode, application functions
can only implement externs declared `@[extern "symbol"]`.

### LNG0408

**Primitive unsupported on the target.** The program reaches a runtime primitive the target
cannot provide. On WebAssembly (the TypeScript binding) there are no threads, child processes
or sockets: `IO.asTask` and the other task primitives, `IO.Process`, and networking are
unavailable. The message names the primitive and the declaration using it.

## Code generation

### LNG0501

**Invalid Bridge IR.** The program violates an invariant the backend relies on (scoping,
arities, representations, literals, closure completeness). This indicates a defect in the
worker or its adapter.

### LNG0502

**Unsupported compiler output.** The compiler output contains a construct the backend does
not implement, such as IR struct or union types.

### LNG0503

**Internal code generator error.** An internal invariant of the code generator was violated.
This indicates a lungo defect.

### LNG0504

**Generator plugin failed.** A generator plugin (`lungo-gen-<language>` on `PATH`, see
[plugins](plugins.md)) could not be run, exited unsuccessfully, wrote a response that is not a
valid `GenerateResponse`, or reported errors. The message contains its standard error or its
errors.

## Trust policy

### LNG0601

**Trust policy violation.** An export violates `deny_sorry`, `deny_axioms` or `deny_unsafe`.
The message lists each violation with the offending dependencies.

```text
error[LNG0601]: exported declarations violate the configured trust policy:
  Formal.step depends on `sorry` (deny_sorry)
```
