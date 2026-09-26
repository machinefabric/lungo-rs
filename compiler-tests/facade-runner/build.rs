use patina_build::Mode;

fn main() -> patina_build::Result<()> {
    for (name, mode) in [("pure", Mode::PureRust), ("oracle", Mode::LeanOracle)] {
        patina_build::configure().mode(mode).name(name).compile_lean("../facade")?;
    }
    Ok(())
}
