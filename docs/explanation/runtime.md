# The runtime

Compiled Lean code assumes a runtime: an object model, reference counting, big numbers,
strings, arrays, closures, tasks and IO. patina brings its own, written in Rust. This page
explains how it relates to Lean's.

## A port, not a reinterpretation

`patina-runtime` is a port of Lean's `lean.h` and its C++ runtime. Objects have Lean's
exact layout — a header of reference count, size and tag, followed by fields — and small
values are tagged scalars as in Lean. Reference counting has Lean's semantics, including
objects shared between threads and persistent objects that are never freed. Freeing a large
structure is iterative, as in Lean, so it cannot overflow the stack.

Keeping the layout identical is what makes the rest of the design work. The compiled code can
be generated instruction by instruction, because every instruction means the same thing as in
Lean's C backend. And the facade's conversions read objects the same way whether they come from
the Rust runtime or from Lean's native runtime in `LeanOracle` mode, which is what makes a
call-by-call comparison of the two meaningful.

## Primitives are declared, not assumed

Lean code reaches native functionality through `@[extern]` symbols. Every primitive the
runtime implements is declared with its C-level signature: the representation of each
parameter and whether it is borrowed or owned. When a program uses an extern symbol, code
generation checks the primitive's signature against Lean's declaration and reconciles
borrowing with explicit reference-count operations, so the two cannot silently drift apart.

An extern symbol is resolved, in order, to:

1. a Lean definition that `@[export]`s the symbol (Lean implements some of its own externs in
   Lean, and those are compiled like any other code);
2. a runtime primitive;
3. a function of the application mapped with `rust_extern`.

Anything else is a build error. The runtime's tests compare it with inventories generated from
the toolchain: every extern symbol of `Init` and `Std` is implemented, and none is shadowed.

## Lean code inside the runtime

Lean's C runtime itself calls a few functions written in Lean: it builds `IO.Error` values
with Lean's constructors, renders them with `IO.Error.toString`, wraps the standard streams
with `IO.FS.Stream.ofHandle`, and prints panics with `IO.eprintln`. The Rust runtime does the
same. Every program includes these compiled Lean definitions and registers them when it
starts. The layout of `IO.Error` therefore lives in exactly one place, the compiled Lean
code, as it does in native Lean.

## Faithful in the corners

Matching Lean means matching behaviour users can observe, not only results. `IO` handles
buffer like C `FILE`s; float formatting follows C's; a child process that cannot run its
program reports the failure from the forked child and exits the way Lean's does, which even
reproduces how C's `exit` flushes output the parent had buffered. The conformance tests
compare standard output, standard error and exit codes byte for byte with Lean's own
executables, which is how such details are found.
