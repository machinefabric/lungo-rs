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
| `LNG06xx` | Trust and assurance policy |
| `LNG07xx` | Assurance records |

For fixing common failures, see
[How to diagnose a failing lungo build](https://machinefabric.com/lungo/docs/how-to/diagnose-build-failures).

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
toolchains; see [platforms](https://machinefabric.com/lungo/docs/reference/platforms).

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
[`name`](https://machinefabric.com/lungo/docs/reference/configuration#program)), two Lean projects in one build script that would generate
the same module, no `OUT_DIR` and no `out_dir` outside a build script, or a
[shaping](https://machinefabric.com/lungo/docs/reference/configuration#shaping-the-generated-code) setting (`type_attribute`,
`struct_attribute`, `enum_attribute`, `field_attribute`, `skip_debug`, `disable_comments`,
`extern_type`) whose path selects nothing.

```text
error[LNG0107]: field_attribute path `Geometry.Point.z` selects no field of a generated type
```

### LNG0108

**lungo runtime unavailable.** A language binding needs the prebuilt lungo runtime, and none
is available: this `lungo` is a development build, which knows no release, and no local
runtime distribution was given with `--runtime-dir`; or the release has no runtime for the
requested target. See [the runtime](https://machinefabric.com/lungo/docs/reference/lungo-cli#the-runtime).

### LNG0109

**Runtime artifact checksum mismatch.** A runtime artifact downloaded by `lungo runtime fetch`
(or found in the cache) does not have the SHA-256 digest recorded in this `lungo` release.
The artifact is not used; the cached copy is removed. A persistent mismatch means the
download was tampered with or corrupted in transit.

### LNG0110

**Generated output is out of date.** `lungo generate --verify` (or `--link`) found an output
directory whose generated sources are not what the project generates now; the message names
every file that changed, is missing, or is extra. Neither the build record (`build-info.json`)
nor a platform product (the TypeScript binding's `program.wasm`) is compared. Run
`lungo generate` and commit the sources it writes.

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
implemented neither by a Lean `@[export]` definition nor by the lungo runtime, and is not an
operation of a capability (`@[lungo_operation C]`), which only the host implements; or it is an
operation the Rust output has no `rust_extern` mapping for. The message names the declaration,
its symbol, Lean type, required representation, and source location.

```text
error[LNG0401]: unresolved Lean external symbol

Declaration: Unknown.providerSend
Symbol: provider_send
Lean type: Nat → Nat
Expected runtime representation: (tobj) -> tobj
Source: lean/Unknown.lean:3:1

Nothing implements it. If the host is to implement it, make it an operation of a capability: give it `@[lungo_operation C]`, `C` being a declaration with `@[lungo_capability "ns.name"]` (lungo's Lean library).
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
[plugins](https://machinefabric.com/lungo/docs/reference/plugins)) could not be run, exited unsuccessfully, wrote a response that is not a
valid `GenerateResponse`, or reported errors. The message contains its standard error or its
errors.

## Trust and assurance policy

### LNG0601

**Trust policy violation.** An export, or the evidence of a claim about one, violates
`deny_sorry`, `deny_axioms` or `deny_unsafe`. The message lists each violation with the
offending dependencies.

```text
error[LNG0601]: exported declarations violate the configured trust policy:
  Formal.step depends on `sorry` (deny_sorry)
```

### LNG0602

**Export without a proved claim.** `require-claims` (`[assurance]` of `lungo.toml`,
`Builder::require_claims`) selects an export that is the subject of no proved claim: no theorem
with `@[lungo_claim … subject <export> …]` whose evidence is free of `sorry`. State what the
export does and prove it, or narrow the policy.

```text
error[LNG0602]: the export Formal.step has no proved claim (`require-claims` selects it with "."); state what it does with `@[lungo_claim]` on a theorem about it
```

### LNG0603

**Forbidden assumption.** A proved claim takes as a hypothesis an assumption `forbid-assumptions`
names (or an assumption of a capability it names), or an export calls an operation of a
capability it names.

## Assurance records

### LNG0701

**Malformed assurance record.** A record (`decl._lungo_…`) is not in the form lungo's Lean
library writes, or names a specification kind, relation, role or capability identifier that is
not a well-formed namespaced string, or not one lungo defines in the `lungo` namespace. Records
are written by the `@[lungo_…]` attributes; one written by hand is read and checked the same way.

### LNG0702

**Dangling assurance reference.** A record names a declaration that does not exist, a claim
cites a specification without `@[lungo_spec]`, or an operation or assumption names something that
is not a capability.

### LNG0703

**Invalid claim.** A claim's evidence is not a theorem, its subject is a theorem, or the
evidence's statement does not have the shape its `lungo.*` relation requires (for
`lungo.decides`, `f … = true ↔ P …`).

### LNG0704

**Claim subject not in its statement.** The statement of a claim's evidence does not mention one
of the claim's subjects or specifications: the theorem is not about what the claim says it is.

### LNG0705

**Duplicate assurance identifier.** Two capabilities have the same identifier.

### LNG0706

**Capability mismatch.** An operation of a capability is not an `@[extern]` declaration with an
entry for C, belongs to an async capability, or has a symbol Lean (`@[export]`) or the lungo
runtime already implements; or a `rust_extern` mapping names an extern that is not an operation
of a capability.

### LNG0707

**Invalid async interface.** An export returns an async program (`Lungo.Async.Program op α`)
whose `Lungo.Async.Interface op` instance is not registered with `@[lungo_capability]`, whose
operations cannot cross to the host, or whose answer type depends on the operation's arguments;
or an async program appears inside a value rather than as what a function returns.

### LNG0708

**Assurance fingerprint mismatch.** `lungo assurance --compose`: two assurance documents
describe a record with the same name (a specification, claim, capability or assumption)
differently: from another Lake package, or with another meaning. The packages were generated
from different definitions; regenerate them from the same Lean sources.

### LNG0709

**Incompatible assurance library.** lungo's Lean library (`Lungo.Registry.schemaVersion`)
writes records in a format this lungo does not read. Use the library released with this lungo.

### LNG0710

**Program depends on an unlinked module's initialization.** The program uses a value that a
module computes when it is initialized (an `initialize` or `[init]` declaration), and lungo does
not link that module: it is loaded only for its assurance records (`assurance-modules`), or
reached only through the compile-time part of lungo's Lean library (`Lungo.Attr`), so nothing
would initialize the value. Import the module from the program's own modules. (Other code of
such a module, which Lean's compiler may reuse, runs as it is.)

### LNG0711

**Invalid assurance document.** An `assurance.json` given to `lungo assurance --compose` cannot be
read, or is of a schema version this lungo does not read.
