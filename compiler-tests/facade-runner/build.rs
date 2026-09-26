use lungo_build::Mode;

fn main() -> lungo_build::Result<()> {
    for (name, mode) in [("pure", Mode::PureRust), ("oracle", Mode::LeanOracle)] {
        lungo_build::configure().mode(mode).name(name).compile_lean("../facade")?;
    }
    Ok(())
}
