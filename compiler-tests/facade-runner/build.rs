use patina_build::{Config, Mode};

fn main() -> patina_build::Result<()> {
    for (dir, mode) in [("ptn-pure", Mode::PureRust), ("ptn-oracle", Mode::LeanOracle)] {
        Config::new("../facade").root_module("Facade").export_module("Facade").mode(mode).output_dir(dir).compile()?;
    }
    Ok(())
}
