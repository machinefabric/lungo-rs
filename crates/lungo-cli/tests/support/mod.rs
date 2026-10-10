//! What the end-to-end tests of generated packages share: running commands, the local
//! distribution they build against, and `lungo generate`.

#![allow(dead_code)] // each test program uses part of it

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, OnceLock};

pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Runs `cmd`, failing the test unless it succeeds; its standard output.
#[track_caller]
pub fn run(cmd: &mut Command) -> String {
    let shown = format!("{cmd:?}");
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {shown}: {e} (is the toolchain installed?)"));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{shown} failed ({}):\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// `dir`, emptied.
pub fn fresh(dir: &Path) -> PathBuf {
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();
    dir.to_path_buf()
}

/// The local distribution under `root` (`<root>/dist`) with `components`, each built once with
/// its own Cargo target directory (`<root>/cargo`; the outer `cargo test` holds the repository's).
pub fn distribution(root: &Path, components: &[&str]) -> PathBuf {
    static BUILT: OnceLock<Mutex<BTreeMap<PathBuf, Vec<String>>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
    let built = built.entry(root.to_path_buf()).or_default();
    let dist = root.join("dist");
    let missing: Vec<&str> = components.iter().copied().filter(|c| !built.iter().any(|b| b == c)).collect();
    if !missing.is_empty() {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.current_dir(repo())
            .env("CARGO_TARGET_DIR", root.join("cargo"))
            .args(["run", "--quiet", "-p", "lungo-dist", "--", "local", "--out"])
            .arg(&dist);
        for c in &missing {
            cmd.args(["--component", c]);
        }
        run(&mut cmd);
        built.extend(missing.iter().map(|c| c.to_string()));
    }
    dist
}

/// Generates the project `config` names for `language` into `out`, against the distribution
/// `dist`.
#[track_caller]
pub fn generate(config: &Path, language: &str, out: &Path, dist: &Path) {
    run(Command::new(env!("CARGO_BIN_EXE_lungo"))
        .arg("--config")
        .arg(config)
        .arg("generate")
        .arg(format!("--{language}_out={}", out.display()))
        .arg("--runtime-dir")
        .arg(dist));
}
