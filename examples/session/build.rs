use lean2rust_build::{Config, Mode};

fn main() -> lean2rust_build::Result<()> {
    Config::new("lean").root_module("Formal.Session").export_module("Formal").mode(Mode::PureRust).compile()
}
