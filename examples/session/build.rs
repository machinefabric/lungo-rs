use patina_build::{Config, Mode};

fn main() -> patina_build::Result<()> {
    Config::new("lean").root_module("Formal.Session").export_module("Formal").mode(Mode::PureRust).compile()
}
