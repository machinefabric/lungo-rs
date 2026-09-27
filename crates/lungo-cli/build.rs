//! Embeds the runtime manifest of a release build.
//!
//! A release's `runtime-manifest.json` lists its runtime archives with their SHA-256 digests;
//! the command downloads and verifies exactly those. The release workflow builds the command
//! with `LUNGO_RUNTIME_MANIFEST` naming the manifest, and publishes the crate with the manifest
//! as `runtime-manifest.json` beside `Cargo.toml`, so that `cargo install lungo-cli` builds a
//! release too. Any other build is a development build, which knows no release and uses a
//! local distribution (`--runtime-dir`).

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=LUNGO_RUNTIME_MANIFEST");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("runtime-manifest.json");
    let packaged = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"))
        .join("runtime-manifest.json");
    println!("cargo::rerun-if-changed={}", packaged.display());
    let source = std::env::var_os("LUNGO_RUNTIME_MANIFEST").map(PathBuf::from).or_else(|| packaged.is_file().then_some(packaged));
    let text = match source {
        Some(path) => {
            println!("cargo::rerun-if-changed={}", path.display());
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read the runtime manifest {}: {e}", path.display()));
            let manifest: serde_json::Value =
                serde_json::from_str(&text).unwrap_or_else(|e| panic!("{} is not valid JSON: {e}", path.display()));
            // The manifest must describe this very version; the command validates the rest when
            // it loads the manifest.
            let version = env!("CARGO_PKG_VERSION");
            if manifest["version"] != version {
                panic!("{} is not the runtime manifest of lungo {version}", path.display());
            }
            text
        }
        None => "null".to_owned(),
    };
    std::fs::write(&out, text).expect("cannot write the embedded runtime manifest");
}
