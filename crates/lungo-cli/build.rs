//! Embeds the runtime manifest of a release build.
//!
//! A release's `runtime-manifest.json` lists its runtime archives with their SHA-256 digests;
//! the command downloads and verifies exactly those. A release builds the command with
//! `LUNGO_RUNTIME_MANIFEST` naming the manifest (`lungo-dist release-cli`), and publishes the
//! crate with the manifest as `runtime-manifest.json` beside `Cargo.toml`, so that
//! `cargo install lungo-cli` builds a
//! release too. Any other build is a development build, which knows no release and uses a
//! local distribution (`--runtime-dir`).
//!
//! A release published in a release registry is also given its **release line**: the anchor of
//! the registry, the channel it is published on, the signing environment, the root keys every
//! release of that line chains to, and MachineFabric's charter roots, which the registry's
//! charter chains to. `lungo self update` reads it back (`src/release.rs`). `MFR_RELEASE_CHANNEL`
//! is what makes a build one of these, and then the others are required with it; a build without
//! it — `cargo install lungo-cli` among them — has no release line and does not update itself.

use std::path::PathBuf;

const CHANNEL: &str = "MFR_RELEASE_CHANNEL";
const RELEASE_LINE: [&str; 5] = [
    "MFR_RELEASES_REGISTRY_URL",
    CHANNEL,
    "MFR_SIGNING_ENVIRONMENT",
    "MFR_CARTRIDGE_ROOT_PUBKEYS",
    "MFR_CHARTER_ROOT_PUBKEYS",
];
/// The roots a release-key certificate is checked against: two of them must have signed it, so
/// fewer than three leaves no root that can be lost or withheld.
const MIN_ROOTS: usize = 3;

fn main() {
    release_line();
    println!("cargo::rerun-if-env-changed=LUNGO_RUNTIME_MANIFEST");
    let out = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("runtime-manifest.json");
    let packaged = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"))
        .join("runtime-manifest.json");
    println!("cargo::rerun-if-changed={}", packaged.display());
    let source = std::env::var_os("LUNGO_RUNTIME_MANIFEST")
        .map(PathBuf::from)
        .or_else(|| packaged.is_file().then_some(packaged));
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

/// Bakes the release line of a registry release as `LUNGO_BAKED_RELEASE_LINE`: the registry's
/// anchor, channel, signing environment, the roots and the charter roots (each comma-separated),
/// with a space between them — one value, so that the line is there whole or not at all. Every
/// part is checked here: a mistyped root stops the build, not every update of what was built.
fn release_line() {
    let given: Vec<(&str, String)> = RELEASE_LINE
        .iter()
        .map(|name| {
            println!("cargo::rerun-if-env-changed={name}");
            (*name, std::env::var(name).unwrap_or_default().trim().to_owned())
        })
        .collect();
    let value = |name: &str| given.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).expect("a listed name");
    if value(CHANNEL).is_empty() {
        return;
    }
    let missing: Vec<&str> = given.iter().filter(|(_, v)| v.is_empty()).map(|(n, _)| *n).collect();
    if !missing.is_empty() {
        panic!(
            "a registry release of lungo ({CHANNEL} is set) is given {} with it; this one is not given {}",
            RELEASE_LINE.iter().filter(|n| **n != CHANNEL).copied().collect::<Vec<_>>().join(", "),
            missing.join(", ")
        );
    }
    let url = value("MFR_RELEASES_REGISTRY_URL");
    if !url.starts_with("https://") || url.ends_with('/') || url.contains(' ') {
        panic!("MFR_RELEASES_REGISTRY_URL must be an https URL with no space and no trailing slash, got {url:?}");
    }
    let channel = value(CHANNEL);
    if !matches!(channel.as_str(), "release" | "nightly") {
        panic!("{CHANNEL} must be `release` or `nightly`, got {channel:?}");
    }
    let environment = value("MFR_SIGNING_ENVIRONMENT");
    if !matches!(environment.as_str(), "prod" | "staging" | "lab") {
        panic!("MFR_SIGNING_ENVIRONMENT must be `prod`, `staging` or `lab` for a registry release, got {environment:?}");
    }
    let roots = root_set("MFR_CARTRIDGE_ROOT_PUBKEYS", &value("MFR_CARTRIDGE_ROOT_PUBKEYS"));
    let charter_roots = root_set("MFR_CHARTER_ROOT_PUBKEYS", &value("MFR_CHARTER_ROOT_PUBKEYS"));
    println!(
        "cargo::rustc-env=LUNGO_BAKED_RELEASE_LINE={url} {channel} {environment} {} {}",
        roots.join(","),
        charter_roots.join(",")
    );
}

/// A root set as `name` gives it: distinct minisign public keys, at least [`MIN_ROOTS`] of them.
fn root_set(name: &str, given: &str) -> Vec<String> {
    let roots: Vec<String> = given.split(',').map(|k| k.trim().to_owned()).collect();
    if roots.iter().any(String::is_empty) {
        panic!("{name} has an empty entry: {roots:?}");
    }
    let mut distinct = roots.clone();
    distinct.sort();
    distinct.dedup();
    if distinct.len() != roots.len() {
        panic!("{name} names a root twice: {roots:?}");
    }
    if roots.len() < MIN_ROOTS {
        panic!("{name} has {} root(s); a certificate needs two of at least {MIN_ROOTS}", roots.len());
    }
    for root in &roots {
        if let Err(why) = minisign_public_key(root) {
            panic!("{name} entry {root:?} is not a minisign public key: {why}");
        }
    }
    roots
}

/// A minisign public key is 42 bytes in base64: `Ed`, an 8-byte key id, a 32-byte ed25519 key.
fn minisign_public_key(text: &str) -> Result<(), String> {
    let bytes = base64(text).ok_or("not base64")?;
    if bytes.len() != 42 {
        return Err(format!("{} bytes, not 42", bytes.len()));
    }
    if &bytes[..2] != b"Ed" {
        return Err("it does not start with the ed25519 tag `Ed`".into());
    }
    Ok(())
}

fn base64(text: &str) -> Option<Vec<u8>> {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let text = text.trim_end_matches('=');
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes() {
        let v = ALPHABET.iter().position(|a| *a == c)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}
