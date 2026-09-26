//! Generates every executable declaration of Lean's `Init` and `Std` (see the `stdlib` fixture of
//! the release-gate tests) as Rust.

fn main() -> patina_build::Result<()> {
    patina_build::Config::new("../gates/fixtures/stdlib").root_module("StdlibCorpus").compile()
}
