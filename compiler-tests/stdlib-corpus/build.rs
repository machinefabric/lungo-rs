//! Generates every executable declaration of Lean's `Init` and `Std` (see the `stdlib` fixture of
//! the release-gate tests) as Rust.

fn main() -> patina_build::Result<()> {
    // The corpus references `sorryAx`, one of Init's executable constants.
    patina_build::configure().deny_sorry(false).compile_lean("../gates/fixtures/stdlib")
}
