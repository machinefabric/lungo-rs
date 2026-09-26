# The compiler boundary

lungo takes Lean's program at the point where Lean's compiler is done with it. This page
explains what that point is for Lean 4.34.1, and what follows from choosing it.

## What Lean hands over

Lean's compiler lowers definitions through several stages. Its final stage for code
generation, impure LCNF, is lowered once more into `Lean.IR`, the representation Lean's own
C backend prints. By then Lean has inferred which parameters are borrowed, boxed and unboxed
values, inserted reference-count operations, and turned opportunities for in-place updates
into explicit checks.

Lean 4.34.1 persists only this last form. Postponed LCNF compilation is off by default, so no
LCNF survives in the compiled files; `Lean.IR` does, in `.ir` files for files using the module
system and in `.olean` files for others. The worker therefore reads `Lean.IR` for every module
in the program's import closure, `Init` and `Std` included, and encodes it as Bridge IR.

Bridge IR represents `Lean.IR` instruction for instruction:

- statements: `let`, `join`, `set`, `set_tag`, `uset`, `sset`, `inc`, `dec`, `del`
- terminators: `case`, `ret`, `jmp`, `unreachable`
- expressions: `ctor`, `reset`, `reuse`, `proj`, `uproj`, `sproj`, `fap`, `pap`, `ap`, `box`,
  `unbox`, literals, `is_shared`

The only structural change is that straight-line code is flattened into blocks, so the Rust
side recurses only on real nesting (join points and case alternatives).

## Consequences

**Nothing is translated twice.** Decisions about borrowing, boxing and reference counting are
Lean's, and the Rust code carries them out as Lean's C backend would. The generated Rust has
the same memory behaviour as native Lean, including destructive updates of unshared arrays.

**Some Bridge IR is never produced.** Lean 4.34.1 expands every `reset`/`reuse` pair into
`is_shared`, `set`, `set_tag` and `del` before the IR is final, so `reset` and `reuse` never
reach lungo from this toolchain. They stay in Bridge IR because they are part of Lean's IR
and another release may emit them; the backend implements them. lungo's tests check both
that every instruction this toolchain emits is exercised and that the set it never emits is
exactly these two.

**Programs are closed over their dependencies.** Because the compiled code of `Init` and
`Std` is itself `Lean.IR`, the standard library goes through the same path as the project:
there is no separate Rust implementation of `List.map` or `String.splitOn`, only of the
runtime primitives underneath.

**The boundary is versioned.** The worker's reading of `Lean.IR` is an adapter for one Lean
release, with its own version number, independent of the Bridge IR version and of the
protocol between worker and build. A change in Lean's IR changes the adapter, not the Rust
backend, unless the IR gains new instructions.

## What belongs to the program

Lean's C backend initializes a module by initializing its imports and running its `[init]`
declarations, in phases: a module using the module system runs a runtime phase that leaves out
its `meta` imports and `meta` declarations, which exist only at compile time. The worker
follows the same rule to decide which modules are part of the program. A project may
therefore use the whole Lean elaborator in a `meta import`ed metaprogram without that code
reaching the Rust binary.
