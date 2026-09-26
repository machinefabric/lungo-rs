//! Differential conformance: every program runs through Lean's official native backend and
//! through patina's PureRust backend, and both must produce the same standard output,
//! standard error, and exit status.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn conformance_project() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance")
}

fn lake() -> PathBuf {
    let home = std::env::var_os("ELAN_HOME").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap()).join(".elan")
    });
    home.join("toolchains/leanprover--lean4---v4.34.1/bin").join(format!("lake{}", std::env::consts::EXE_SUFFIX))
}

fn programs() -> Vec<String> {
    let text = std::fs::read_to_string(conformance_project().join("lakefile.toml")).unwrap();
    let config: toml::Table = toml::from_str(&text).unwrap();
    config["lean_exe"].as_array().unwrap().iter().map(|exe| exe["name"].as_str().unwrap().to_owned()).collect()
}

/// Builds every executable of the conformance project with Lean's native backend.
fn build_native(programs: &[String]) {
    let status = Command::new(lake()).arg("build").args(programs).current_dir(conformance_project()).status().unwrap();
    assert!(status.success(), "the native conformance build failed");
}

fn native_exe(name: &str) -> PathBuf {
    conformance_project().join(".lake/build/bin").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

/// Arguments, environment and working directory shared by both runs of a program. The
/// `process` program spawns the natively built `child` helper in both runs.
fn configure(cmd: &mut Command) -> &mut Command {
    cmd.args(["first", "second arg", "ünïcode"])
        .current_dir(conformance_project())
        .env("LEAN_BACKTRACE", "0")
        .env("PATINA_CONFORMANCE_VAR", "present")
        .env("PATINA_CONFORMANCE_CHILD", native_exe("child"))
        .env_remove("LEAN_ABORT_ON_PANIC")
}

fn run_native(name: &str) -> Output {
    let exe = native_exe(name);
    configure(&mut Command::new(&exe)).output().unwrap_or_else(|e| panic!("cannot run {}: {e}", exe.display()))
}

fn run_pure_rust(name: &str) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_conformance"));
    cmd.arg(name);
    configure(&mut cmd).output().unwrap()
}

#[test]
fn pure_rust_matches_the_native_lean_backend() {
    let programs = programs();
    build_native(&programs);
    let mut failures = Vec::new();
    for name in programs {
        let native = run_native(&name);
        let ours = run_pure_rust(&name);
        let same =
            native.stdout == ours.stdout && native.stderr == ours.stderr && native.status.code() == ours.status.code();
        if !same {
            failures.push(format!(
                "== {name}\n-- native (status {:?})\n{}\n-- stderr\n{}\n-- pure rust (status {:?})\n{}\n-- stderr\n{}",
                native.status.code(),
                String::from_utf8_lossy(&native.stdout),
                String::from_utf8_lossy(&native.stderr),
                ours.status.code(),
                String::from_utf8_lossy(&ours.stdout),
                String::from_utf8_lossy(&ours.stderr),
            ));
        }
    }
    assert!(failures.is_empty(), "{} programs differ:\n{}", failures.len(), failures.join("\n"));
}
