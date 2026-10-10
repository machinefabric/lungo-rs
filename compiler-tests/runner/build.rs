//! Generates every executable of the conformance Lake project through lungo's Rust backend and
//! through its C backend, whose C is compiled and linked against the runtime's C library.

use std::fmt::Write;
use std::path::{Path, PathBuf};

fn main() -> lungo_build::Result<()> {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by Cargo"));
    let out = PathBuf::from(std::env::var("OUT_DIR").expect("set by Cargo"));
    let project = Path::new("../conformance");
    let lakefile = manifest.join(project).join("lakefile.toml");
    println!("cargo::rerun-if-changed={}", lakefile.display());
    let text = std::fs::read_to_string(&lakefile).expect("the conformance lakefile exists");
    let config: toml::Table = toml::from_str(&text).expect("the conformance lakefile is valid TOML");
    let exes = config["lean_exe"].as_array().expect("the conformance project declares executables");
    let env = lungo_build::Environment::native(manifest.clone(), out.join("c-analysis"), out.join("c-work"));
    let mut dispatch = String::new();
    let mut rust_arms = String::new();
    let mut c_arms = String::new();
    for exe in exes {
        let name = exe["name"].as_str().expect("executable name");
        let root = exe["root"].as_str().expect("executable root module");
        let cfg = lungo_build::configure().root_module(root).name(name);
        cfg.clone().compile_lean(project)?;
        writeln!(dispatch, "#[allow(dead_code)]\nmod p_{name} {{ lungo::include_lean!({name:?}); }}").unwrap();
        writeln!(rust_arms, "        {name:?} => Some(p_{name}::__lean_main_with(args)),").unwrap();

        let analysis = cfg.analyze(project, &env)?;
        let program = lungo_build::codegen::c::generate_program(&lungo_build::codegen::c::ProgramInput {
            success: &analysis.success,
            toolchain: &analysis.toolchain,
            name,
            local_prefix: "../conformance",
            target: &std::env::var("TARGET").expect("Cargo sets TARGET"),
        })
        .unwrap_or_else(|errors| {
            let text: Vec<String> = errors.iter().map(|e| e.to_string()).collect();
            panic!("the C backend rejected {name}:\n{}", text.join("\n"))
        });
        let dir = out.join("c").join(name);
        if dir.exists() {
            std::fs::remove_dir_all(&dir).expect("removable generated C");
        }
        let mut build = cc::Build::new();
        build.include(dir.join(lungo_build::codegen::c::PROGRAM_DIR));
        for (path, text) in &program.files {
            let file = dir.join(path);
            std::fs::create_dir_all(file.parent().unwrap()).expect("writable OUT_DIR");
            std::fs::write(&file, text).expect("writable OUT_DIR");
            if path.ends_with(".c") {
                build.file(&file);
            }
        }
        build.compile(&format!("conformance_c_{name}"));
        let run_main = program.run_main.as_deref().expect("conformance programs define `main`");
        writeln!(
            dispatch,
            "unsafe extern \"C\" {{ fn {run_main}(argc: usize, argv: *const *const ::std::ffi::c_char) -> i32; }}"
        )
        .unwrap();
        writeln!(c_arms, "        {name:?} => Some(unsafe {{ {run_main}(argv.len(), argv.as_ptr()) }}),").unwrap();
    }
    writeln!(
        dispatch,
        "/// Runs the conformance program `name` compiled by the Rust backend.\npub fn run_rust(name: &str, args: Vec<String>) -> Option<i32> {{\n    match name {{\n{rust_arms}        _ => None,\n    }}\n}}"
    )
    .unwrap();
    writeln!(
        dispatch,
        "/// Runs the conformance program `name` compiled by the C backend.\npub fn run_c(name: &str, args: Vec<String>) -> Option<i32> {{\n    let args: Vec<::std::ffi::CString> = args.into_iter().map(|a| ::std::ffi::CString::new(a).expect(\"arguments contain no NUL\")).collect();\n    let argv: Vec<*const ::std::ffi::c_char> = args.iter().map(|a| a.as_ptr()).collect();\n    match name {{\n{c_arms}        _ => None,\n    }}\n}}"
    )
    .unwrap();
    std::fs::write(out.join("programs.rs"), dispatch).expect("writable OUT_DIR");
    Ok(())
}
