use lean2rust_build::{Config, Mode};

fn main() -> lean2rust_build::Result<()> {
    for (dir, mode) in [("l2r-pure", Mode::PureRust), ("l2r-oracle", Mode::LeanOracle)] {
        Config::new("../facade").root_module("Facade").export_module("Facade").mode(mode).output_dir(dir).compile()?;
    }
    Ok(())
}
