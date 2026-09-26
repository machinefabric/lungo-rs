//! Design §38: Lean's standard library compiles through the bridge to the extent it has
//! executable code. The `stdlib` fixture references every constant with compiled runtime code in
//! `Init` and `Std`; its complete program is analyzed by the worker, verified, and translated to
//! Rust.

use patina_build::{Environment, configure};
use std::path::Path;

#[test]
fn the_executable_standard_library_translates() {
    let project = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/stdlib").canonicalize().unwrap();
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join("stdlib");
    let env = Environment::native(project.clone(), scratch.join("out"), scratch.join("work"));
    // The corpus references `sorryAx`, one of Init's executable constants.
    let cfg = configure().deny_sorry(false);
    let analysis = cfg.analyze(&project, &env).unwrap();
    assert_eq!(analysis.success.root_modules, ["StdlibCorpus"], "the default target's root");
    let program = &analysis.success.bir;

    // The corpus generator is compile-time code: neither it nor the Lean compiler it uses is
    // part of the program.
    let modules: Vec<&str> = program.modules.iter().map(|m| m.name.as_str()).collect();
    assert!(!modules.contains(&"StdlibCorpus.Generate"), "meta imports are not linked");
    assert!(!modules.iter().any(|m| m.starts_with("Lean.Elab")), "the elaborator is not linked");

    // The whole executable library is in the program (about 10,000 compiled declarations from
    // Init and 40,000 from Std with Lean 4.34.1); the bound guards against a shrinking corpus.
    let count = |prefix: &str| program.declarations.iter().filter(|d| d.module.starts_with(prefix)).count();
    let (init, std) = (count("Init"), count("Std"));
    assert!(init > 8_000 && std > 30_000, "Init: {init} declarations, Std: {std} declarations");

    patina_build::bir::validate(program).unwrap_or_else(|errors| {
        panic!("{} verifier errors, first: {}", errors.len(), errors[0]);
    });
    let generated = cfg.generate(&project, &env, &analysis).unwrap();
    let rust_bytes: usize = generated.files.iter().filter(|(k, _)| k.ends_with(".rs")).map(|(_, v)| v.len()).sum();
    assert!(rust_bytes > 10_000_000, "generated {rust_bytes} bytes of Rust");
}
