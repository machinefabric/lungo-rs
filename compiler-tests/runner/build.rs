//! Generates every executable of the conformance Lake project through patina.

use patina_build::Config;
use std::fmt::Write;

fn main() -> patina_build::Result<()> {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo");
    let lakefile = std::path::Path::new(&manifest).join("../conformance/lakefile.toml");
    println!("cargo::rerun-if-changed={}", lakefile.display());
    let text = std::fs::read_to_string(&lakefile).expect("the conformance lakefile exists");
    let config: toml::Table = toml::from_str(&text).expect("the conformance lakefile is valid TOML");
    let exes = config["lean_exe"].as_array().expect("the conformance project declares executables");
    let mut dispatch = String::new();
    let mut arms = String::new();
    for exe in exes {
        let name = exe["name"].as_str().expect("executable name");
        let root = exe["root"].as_str().expect("executable root module");
        Config::new("../conformance")
            .root_module(root)
            .output_dir(format!("ptn-{name}"))
            .output_name("program")
            .compile()?;
        writeln!(
            dispatch,
            "#[allow(dead_code)]\nmod p_{name} {{ include!(concat!(env!(\"OUT_DIR\"), \"/ptn-{name}/program.rs\")); }}"
        )
        .unwrap();
        writeln!(arms, "        {name:?} => Some(p_{name}::__lean_main_with(args)),").unwrap();
    }
    writeln!(
        dispatch,
        "/// Runs the conformance program `name`.\npub fn run(name: &str, args: Vec<String>) -> Option<i32> {{\n    match name {{\n{arms}        _ => None,\n    }}\n}}"
    )
    .unwrap();
    let out = std::env::var("OUT_DIR").expect("set by Cargo");
    std::fs::write(std::path::Path::new(&out).join("programs.rs"), dispatch).expect("writable OUT_DIR");
    Ok(())
}
