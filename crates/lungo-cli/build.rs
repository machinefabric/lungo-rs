//! Embeds the runtime manifest of a release build.
//!
//! The release workflow builds the `lungo` command with `LUNGO_RUNTIME_MANIFEST` naming the
//! release's `runtime-manifest.json` (the runtime archives with their SHA-256 digests); the
//! command then downloads and verifies exactly those. Any other build is a development build,
//! which knows no release and uses a local distribution (`--runtime-dir`).

use std::path::PathBuf;

fn main() {
    println!("cargo::rerun-if-env-changed=LUNGO_RUNTIME_MANIFEST");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("runtime-manifest.json");
    let text = match std::env::var_os("LUNGO_RUNTIME_MANIFEST") {
        Some(path) => {
            let path = PathBuf::from(path);
            println!("cargo::rerun-if-changed={}", path.display());
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("cannot read LUNGO_RUNTIME_MANIFEST {}: {e}", path.display()));
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
