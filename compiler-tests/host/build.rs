use patina_build::{Config, Mode};

fn main() -> patina_build::Result<()> {
    Config::new("lean")
        .root_module("Host")
        .export_module("Host.Syntax")
        .export_module("Host.Callbacks")
        .mode(Mode::PureRust)
        .rust_extern("host_lookup", "crate::host::lookup")
        .rust_extern("host_log", "crate::host::log")
        .rust_extern("host_transform", "crate::host::transform")
        .compile()
}
