# Trust and verification

A Lean proof says something about a Lean definition. Running that definition as Rust adds
links to the chain between the proof and the running program. This page describes those
links, how each is checked, and what the build isolates and what it does not.

## What a proof covers

Lean's kernel checks proofs during `lake build`, before patina sees anything. patina
adds no reasoning of its own: a theorem about `Shapes.area` holds for the Rust `area` exactly
insofar as the Rust code computes what the Lean definition computes. The question is
therefore how much one has to trust to believe that it does.

## The trusted base

- **The Lean toolchain.** Its elaborator, kernel and compiler, as `lake build` runs them.
  Anyone running Lean code natively trusts the same components.
- **The worker's adapter.** It reads the compiler's output and metadata and encodes them; it
  transforms no code.
- **The Rust backend.** The translation of each Bridge IR instruction, the facade's
  conversions, and the runtime's primitives.

The first is Lean's responsibility. The other two are patina's, and each is checked from a
different side.

## How the backend is checked

**Independent verification of the input.** Before generating code, patina checks the
Bridge IR on its own terms: variables and join points in scope, calls with the right number
and representation of arguments, literals in range, every called declaration present. A
defect in the worker surfaces as a verification error instead of wrong Rust.

**Comparison with Lean's own backend.** The conformance programs are compiled twice, as Rust
and by Lean's native backend, and their output, error output and exit codes must agree byte
for byte. The corpus covers every instruction the toolchain emits. Separately, a library of
exported functions runs on both backends in one process, compared call by call over generated
inputs: large numbers, Unicode text, floats including NaN and negative zero, trees, closures in
both directions, IO errors.

**Ties to the toolchain.** Every extern symbol of the standard library is accounted for with
the representation Lean declares, and the complete executable standard library, about fifty
thousand compiled declarations, passes verification and generates Rust that type-checks.

None of this is a proof of the backend. It is the level of assurance of a compiler that is
tested against a reference implementation, and the reference is Lean's own.

## Trust metadata

Some Lean code is outside what proofs vouch for: definitions that use `sorry`, axioms beyond
Lean's standard three, `unsafe` code, `partial` definitions, and native externs. patina
computes, for every export, which of these its executable code depends on, records it in the
generated metadata, and can refuse to build when an export depends on `sorry`, extra axioms
or `unsafe` code. This makes the assumptions behind a Rust function visible where it is used.

## What the build isolates

Building a Lean project runs its code: macros, elaborators and tactics run during `lake build`,
and the worker loads the project's compiled code. Adding a Lean dependency is therefore like
adding a Cargo build dependency or procedural macro: it runs with your permissions.

The worker runs as a separate process under supervision. A crash, a hang or a malformed
response is a build error, never a failure of Cargo itself; a timeout kills every process the
worker started. Optional limits bound its processor time and memory where the operating system
can enforce them, and a hermetic mode withholds your environment variables. These contain
failures and runaway builds. They are not a sandbox: file-system and network access are not
restricted, and a malicious dependency is not contained.
