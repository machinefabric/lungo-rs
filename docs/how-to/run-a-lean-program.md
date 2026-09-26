# How to run a Lean program's `main` from Rust

This guide shows how to build a Lean executable, one with a `main`, as a Rust binary.

## Generate the program

The module that defines `main` must be a root module. A Lake executable that is a default
target already makes its `root` one, so a build of the package's default targets needs no
configuration. `main` may have any of Lean's forms: `IO Unit`, `IO UInt32`,
`List String → IO Unit`, or `List String → IO UInt32`.

```rust
// build.rs
fn main() -> lungo_build::Result<()> {
    lungo_build::configure().root_module("Tool.Main").name("program").compile_lean("lean")
}
```

The generated module then has `__lean_main()` and `__lean_main_with(args)`.

## Call it

```rust
// src/main.rs
mod program {
    lungo::include_lean!("program");
}

fn main() {
    std::process::exit(program::__lean_main());
}
```

`__lean_main()` passes the process arguments (without the program name) to Lean's `main`,
runs it on a thread with the stack size Lean programs expect, and returns the exit code: the
`UInt32` Lean returned, `0` for `Unit`, or `1` after printing an uncaught `IO` error as Lean
does (`uncaught exception: …`).

To pass other arguments, call `program::__lean_main_with(vec!["--flag".into()])`.

## Several programs in one binary

Give each program its own name:

```rust
// build.rs
fn main() -> lungo_build::Result<()> {
    for (name, root) in [("fmt", "Tool.Fmt"), ("check", "Tool.Check")] {
        lungo_build::configure().root_module(root).name(name).compile_lean("lean")?;
    }
    Ok(())
}
```

and dispatch on it:

```rust
// src/main.rs
mod fmt {
    lungo::include_lean!("fmt");
}
mod check {
    lungo::include_lean!("check");
}

fn main() {
    let mut args = std::env::args().skip(1);
    let code = match args.next().as_deref() {
        Some("fmt") => fmt::__lean_main_with(args.collect()),
        Some("check") => check::__lean_main_with(args.collect()),
        _ => 2,
    };
    std::process::exit(code);
}
```

`compiler-tests/runner` in the repository generates every executable of a Lake project this
way.
