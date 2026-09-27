//! The TypeScript binding's WebAssembly module: the program's C and the runtime for
//! `wasm32-wasip1`, linked by the pinned wasi-sdk.

use crate::runtime::download_verified;
use lungo_build::codegen::c::boundary::Boundary;
use lungo_build::{Error, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The wasi-sdk release `lungo` links with.
pub const WASI_SDK_VERSION: &str = "34.0";

/// The wasi-sdk archive for this host: name and SHA-256 (from the wasi-sdk release).
fn wasi_sdk_archive() -> Result<(&'static str, &'static str)> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => ("arm64-macos", "9c59398106b417f8f14913380fdf0097a8cc0ff4af9eb3ce0065a859e88d49e9"),
        ("macos", "x86_64") => ("x86_64-macos", "87d27fa8adc68dee59bfbf2e22a6d34ef717c34d6bf1d8af2a56fc929d9ce0eb"),
        ("linux", "x86_64") => ("x86_64-linux", "b761e3a0721dbae9c09a0059e5fdb2bf917d1b4a8a7b430fb3b5aafb0984b2c4"),
        ("linux", "aarch64") => ("arm64-linux", "f7e243dff54d60bcc576e94d6166b69f410f2500ae4a9ceef34315be10e77971"),
        ("windows", "x86_64") => ("x86_64-windows", "cccb5c323a9b34f0349a9b09e8804a0a7632c68c3310f4b5f437ed57d7e71d8f"),
        ("windows", "aarch64") => ("arm64-windows", "45e1c71f3e965621e7b98ebe1d37b0e4b1f77f3e8072113ffb4534e67b1a4b7c"),
        (os, arch) => {
            return Err(Error::Environment(format!(
                "wasi-sdk has no release for {os}/{arch}; install wasi-sdk {WASI_SDK_VERSION} and pass --wasi-sdk"
            )));
        }
    })
}

/// The wasi-sdk to link with: `explicit` (`--wasi-sdk`, else `WASI_SDK_PATH`), else the pinned
/// release, downloaded into the cache and verified.
pub fn wasi_sdk(explicit: Option<&Path>) -> Result<PathBuf> {
    let explicit = explicit.map(Path::to_path_buf).or_else(|| std::env::var_os("WASI_SDK_PATH").map(PathBuf::from));
    if let Some(dir) = explicit {
        if !clang(&dir).is_file() {
            return Err(Error::Environment(format!("{} is not a wasi-sdk (no bin/clang)", dir.display())));
        }
        return Ok(dir);
    }
    let (host, sha256) = wasi_sdk_archive()?;
    let dir = lungo_build::cache_root()?.join("wasi-sdk").join(WASI_SDK_VERSION).join(host);
    let verified = dir.join(".lungo-verified");
    if std::fs::read_to_string(&verified).is_ok_and(|d| d.trim() == sha256) {
        return Ok(dir);
    }
    let major = WASI_SDK_VERSION.split('.').next().expect("a version has a major part");
    let url = format!(
        "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-{major}/wasi-sdk-{WASI_SDK_VERSION}-{host}.tar.gz"
    );
    download_verified(&url, sha256, &dir)?;
    Ok(dir)
}

fn clang(sdk: &Path) -> PathBuf {
    sdk.join("bin").join(format!("clang{}", std::env::consts::EXE_SUFFIX))
}

/// The symbols the module exports: the program's boundary and the runtime's WebAssembly host
/// interface.
fn exports(boundary: &Boundary) -> Vec<String> {
    let mut out: Vec<String> = boundary.functions.iter().map(|f| f.symbol.clone()).collect();
    out.push(boundary.types_symbol.clone());
    out.push(boundary.set_host_extern.clone());
    out.push(boundary.initialize.clone());
    out.extend(boundary.run_main.iter().cloned());
    out.extend(lungo_build::codegen::c::WASM_RUNTIME_EXPORTS.iter().map(|s| s.to_string()));
    out
}

/// Compiles the program's C (`program_files`, paths under `program/`) and links it with the
/// `wasm32-wasip1` runtime package `runtime` into a WebAssembly reactor module.
pub fn link(
    program_files: &BTreeMap<String, String>,
    boundary: &Boundary,
    runtime: &Path,
    sdk: &Path,
    work: &Path,
) -> Result<Vec<u8>> {
    let src = work.join("wasm-src");
    if src.exists() {
        std::fs::remove_dir_all(&src).map_err(|e| Error::io(format!("cannot clear {}", src.display()), e))?;
    }
    let mut sources = Vec::new();
    for (rel, text) in program_files {
        let path = src.join(rel);
        std::fs::create_dir_all(path.parent().expect("files have a parent"))
            .map_err(|e| Error::io(format!("cannot create {}", src.display()), e))?;
        std::fs::write(&path, text).map_err(|e| Error::io(format!("cannot write {}", path.display()), e))?;
        if rel.ends_with(".c") {
            sources.push(path);
        }
    }
    let library = runtime.join("lib").join("liblungo.a");
    if !library.is_file() {
        return Err(Error::RuntimeUnavailable(format!(
            "{} is not a wasm32-wasip1 lungo runtime (no lib/liblungo.a)",
            runtime.display()
        )));
    }
    let out = work.join("program.wasm");
    let mut cmd = Command::new(clang(sdk));
    cmd.arg("--target=wasm32-wasip1")
        .arg(format!("--sysroot={}", sdk.join("share").join("wasi-sysroot").display()))
        .args(["-O2", "-std=c11", "-mexec-model=reactor", "-Wl,--no-entry"])
        .arg("-I")
        .arg(src.join("program"))
        .args(&sources)
        .arg(&library)
        .arg("-o")
        .arg(&out);
    for symbol in exports(boundary) {
        cmd.arg(format!("-Wl,--export={symbol}"));
    }
    let output = cmd.output().map_err(|e| Error::io(format!("cannot run {}", clang(sdk).display()), e))?;
    if !output.status.success() {
        return Err(Error::Command {
            program: "clang --target=wasm32-wasip1".into(),
            status: output.status.to_string(),
            output: format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr)),
        });
    }
    std::fs::read(&out).map_err(|e| Error::io(format!("cannot read {}", out.display()), e))
}
