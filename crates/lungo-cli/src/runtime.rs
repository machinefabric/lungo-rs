//! The prebuilt lungo runtime: which one generated packages use, and verified downloads.
//!
//! A release of `lungo` embeds the release's runtime manifest (every runtime archive with its
//! SHA-256 digest); generated packages refer to those archives, and `lungo runtime fetch`
//! downloads them into the cache, verifying each digest before use. A development build knows no
//! release: it uses a local distribution (`--runtime-dir`), laid out like the release.

use lungo_build::codegen::plugin::{Artifact, Distribution, RuntimeInfo};
use lungo_build::{Error, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const EMBEDDED_MANIFEST: &str = include_str!(concat!(env!("OUT_DIR"), "/runtime-manifest.json"));

/// A release's runtime manifest (`runtime-manifest.json`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub version: String,
    pub abi_version: u32,
    /// Runtime archives by target triple, and the Apple XCFramework (`xcframework`).
    pub artifacts: BTreeMap<String, Artifact>,
}

impl Manifest {
    /// Parses and validates a manifest for this `lungo`.
    pub fn parse(text: &str) -> Result<Manifest> {
        let m: Manifest = serde_json::from_str(text)
            .map_err(|e| Error::Configuration(format!("the runtime manifest is malformed: {e}")))?;
        let version = env!("CARGO_PKG_VERSION");
        if m.version != version {
            return Err(Error::Configuration(format!(
                "the runtime manifest is for lungo {}, not {version}",
                m.version
            )));
        }
        if m.abi_version != lungo_build::RUNTIME_ABI_VERSION {
            return Err(Error::Configuration(format!(
                "the runtime manifest is for C ABI {}, not {}",
                m.abi_version,
                lungo_build::RUNTIME_ABI_VERSION
            )));
        }
        for (target, a) in &m.artifacts {
            if a.sha256.len() != 64 || !a.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                return Err(Error::Configuration(format!("the runtime manifest's digest of {target} is not SHA-256")));
            }
            if !a.url.starts_with("https://") {
                return Err(Error::Configuration(format!("the runtime manifest's URL of {target} is not HTTPS")));
            }
        }
        Ok(m)
    }
}

/// The manifest this `lungo` was released with, or `None` for a development build.
pub fn embedded_manifest() -> Result<Option<Manifest>> {
    if EMBEDDED_MANIFEST.trim() == "null" { Ok(None) } else { Manifest::parse(EMBEDDED_MANIFEST).map(Some) }
}

/// Where generated packages get the runtime: the local distribution `runtime_dir`, or this
/// release's.
pub fn select(runtime_dir: Option<&Path>) -> Result<RuntimeInfo> {
    let distribution = match runtime_dir {
        Some(dir) => {
            let dir = lungo_build::canonical_path(dir)
                .map_err(|e| Error::io(format!("cannot resolve the runtime distribution {}", dir.display()), e))?;
            check_local(&dir)?;
            Distribution::Local { dir: dir.to_string_lossy().replace('\\', "/") }
        }
        None => match embedded_manifest()? {
            Some(m) => Distribution::Release { artifacts: m.artifacts },
            None => {
                return Err(Error::RuntimeUnavailable(
                    "this lungo is a development build, which knows no runtime release: pass --runtime-dir with a local distribution (built by `lungo-dist local`)".into(),
                ));
            }
        },
    };
    Ok(RuntimeInfo {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        abi_version: lungo_build::RUNTIME_ABI_VERSION,
        distribution,
    })
}

/// A local distribution holds a runtime of exactly this version.
fn check_local(dir: &Path) -> Result<()> {
    let version_file = dir.join("VERSION");
    let version = fs::read_to_string(&version_file).map_err(|e| {
        Error::RuntimeUnavailable(format!(
            "{} is not a lungo distribution (cannot read {}: {e})",
            dir.display(),
            version_file.display()
        ))
    })?;
    if version.trim() != env!("CARGO_PKG_VERSION") {
        return Err(Error::RuntimeUnavailable(format!(
            "{} is a distribution of lungo {}, not {}",
            dir.display(),
            version.trim(),
            env!("CARGO_PKG_VERSION")
        )));
    }
    Ok(())
}

/// The runtime package (`include/`, `lib/`) for `target` in the distribution of `info`:
/// downloaded and verified for a release, from the local distribution otherwise.
pub fn runtime_package(info: &RuntimeInfo, target: &str) -> Result<PathBuf> {
    match &info.distribution {
        Distribution::Local { dir } => {
            let sub = if target == "wasm32-wasip1" { "wasm" } else { "runtime" };
            let path = Path::new(dir).join(sub);
            if !path.join("include").join("lungo.h").is_file() {
                return Err(Error::RuntimeUnavailable(format!(
                    "the local distribution {dir} has no runtime for {target} (expected {})",
                    path.display()
                )));
            }
            Ok(path)
        }
        Distribution::Release { artifacts } => fetch(artifacts, target),
    }
}

fn cache_dir() -> Result<PathBuf> {
    Ok(lungo_build::cache_root()?.join("runtime").join(env!("CARGO_PKG_VERSION")))
}

fn artifact<'a>(artifacts: &'a BTreeMap<String, Artifact>, target: &str) -> Result<&'a Artifact> {
    artifacts.get(target).ok_or_else(|| {
        let available: Vec<&str> = artifacts.keys().map(String::as_str).filter(|k| *k != "xcframework").collect();
        Error::RuntimeUnavailable(format!(
            "lungo {} has no runtime for {target} (available: {})",
            env!("CARGO_PKG_VERSION"),
            available.join(", ")
        ))
    })
}

const VERIFIED: &str = ".lungo-verified";

/// Downloads (unless cached and verified) and unpacks the runtime archive for `target`.
pub fn fetch(artifacts: &BTreeMap<String, Artifact>, target: &str) -> Result<PathBuf> {
    let a = artifact(artifacts, target)?;
    let dir = cache_dir()?.join(target);
    if fs::read_to_string(dir.join(VERIFIED)).is_ok_and(|d| d.trim() == a.sha256) {
        return Ok(dir);
    }
    download_verified(&a.url, &a.sha256, &dir)?;
    Ok(dir)
}

/// The cached runtime for `target`, which must have been fetched.
pub fn cached(artifacts: &BTreeMap<String, Artifact>, target: &str) -> Result<PathBuf> {
    let a = artifact(artifacts, target)?;
    let dir = cache_dir()?.join(target);
    match fs::read_to_string(dir.join(VERIFIED)) {
        Ok(d) if d.trim() == a.sha256 => Ok(dir),
        _ => Err(Error::RuntimeUnavailable(format!(
            "the runtime for {target} is not in the cache; run `lungo runtime fetch --target {target}`"
        ))),
    }
}

/// Downloads the archive of `target` again and checks it against the release's digest.
pub fn verify(artifacts: &BTreeMap<String, Artifact>, target: &str) -> Result<PathBuf> {
    let a = artifact(artifacts, target)?;
    let dir = cache_dir()?.join(target);
    download_verified(&a.url, &a.sha256, &dir)?;
    Ok(dir)
}

/// Downloads the `.tar.gz` at `url`, checks its SHA-256 digest, and unpacks it into `dir`
/// (without its single top-level directory), replacing `dir`. A mismatching download is
/// discarded: nothing of it is used.
pub fn download_verified(url: &str, sha256: &str, dir: &Path) -> Result<()> {
    install_verified(&download(url)?, url, sha256, dir)
}

/// Checks the `.tar.gz` `bytes` (downloaded from `source`) against `sha256` and unpacks it into
/// `dir` as [`download_verified`] does.
fn install_verified(bytes: &[u8], source: &str, sha256: &str, dir: &Path) -> Result<()> {
    let digest = hex(&Sha256::digest(bytes));
    if digest != sha256 {
        return Err(Error::RuntimeChecksum(format!(
            "{source} has SHA-256 {digest}, but this lungo release lists {sha256}; the download was discarded"
        )));
    }
    let parent = dir.parent().expect("cache directories have a parent");
    fs::create_dir_all(parent).map_err(|e| Error::io(format!("cannot create {}", parent.display()), e))?;
    let partial = parent.join(format!(".download-{}", std::process::id()));
    if partial.exists() {
        fs::remove_dir_all(&partial).map_err(|e| Error::io(format!("cannot clear {}", partial.display()), e))?;
    }
    unpack(bytes, &partial).map_err(|e| Error::io(format!("cannot unpack {source}"), e))?;
    let root = single_top_level(&partial)?;
    fs::write(root.join(VERIFIED), format!("{sha256}\n")).map_err(|e| Error::io("cannot record the digest", e))?;
    if dir.exists() {
        fs::remove_dir_all(dir).map_err(|e| Error::io(format!("cannot replace {}", dir.display()), e))?;
    }
    fs::rename(&root, dir).map_err(|e| Error::io(format!("cannot move the download to {}", dir.display()), e))?;
    let _ = fs::remove_dir_all(&partial);
    Ok(())
}

fn download(url: &str) -> Result<Vec<u8>> {
    let failed = |e: &dyn std::fmt::Display| Error::RuntimeUnavailable(format!("cannot download {url}: {e}"));
    let mut response = ureq::get(url).call().map_err(|e| failed(&e))?;
    let mut bytes = Vec::new();
    response.body_mut().as_reader().read_to_end(&mut bytes).map_err(|e| failed(&e))?;
    Ok(bytes)
}

fn unpack(bytes: &[u8], into: &Path) -> std::io::Result<()> {
    fs::create_dir_all(into)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes));
    for entry in archive.entries()? {
        let mut entry = entry?;
        // `unpack_in` refuses paths escaping the directory.
        if !entry.unpack_in(into)? {
            return Err(std::io::Error::other(format!("the archive has an unsafe path {}", entry.path()?.display())));
        }
    }
    Ok(())
}

/// The single top-level directory of an unpacked archive.
fn single_top_level(dir: &Path) -> Result<PathBuf> {
    let entries: Vec<PathBuf> = fs::read_dir(dir)
        .map_err(|e| Error::io(format!("cannot read {}", dir.display()), e))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()
        .map_err(|e| Error::io(format!("cannot read {}", dir.display()), e))?;
    match entries.as_slice() {
        [one] if one.is_dir() => Ok(one.clone()),
        _ => Err(Error::RuntimeUnavailable(format!(
            "the downloaded archive does not hold a single directory ({} entries)",
            entries.len()
        ))),
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The SHA-256 of a file.
pub fn file_sha256(path: &Path) -> Result<String> {
    let mut f = fs::File::open(path).map_err(|e| Error::io(format!("cannot open {}", path.display()), e))?;
    let mut h = Sha256::new();
    let mut buf = [0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
        if n == 0 {
            break;
        }
        h.write_all(&buf[..n]).expect("hashing cannot fail");
    }
    Ok(hex(&h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_text(version: &str, sha: &str, url: &str) -> String {
        format!(
            r#"{{"version": "{version}", "abi_version": {}, "artifacts": {{"x86_64-unknown-linux-gnu": {{"url": "{url}", "sha256": "{sha}"}}}}}}"#,
            lungo_build::RUNTIME_ABI_VERSION
        )
    }

    #[test]
    fn manifests_must_match_this_release_and_carry_sha256_over_https() {
        let v = env!("CARGO_PKG_VERSION");
        let sha = "a".repeat(64);
        assert!(Manifest::parse(&manifest_text(v, &sha, "https://example.com/r.tar.gz")).is_ok());
        assert!(Manifest::parse(&manifest_text("0.0.1", &sha, "https://example.com/r.tar.gz")).is_err());
        assert!(Manifest::parse(&manifest_text(v, "abc", "https://example.com/r.tar.gz")).is_err());
        assert!(Manifest::parse(&manifest_text(v, &sha, "http://example.com/r.tar.gz")).is_err());
    }

    #[test]
    fn a_tampered_archive_is_rejected_and_never_unpacked() {
        let dir = std::env::temp_dir().join(format!("lungo-runtime-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        // A valid archive: one directory with a file.
        let mut tarball = Vec::new();
        {
            let gz = flate2::write::GzEncoder::new(&mut tarball, flate2::Compression::default());
            let mut b = tar::Builder::new(gz);
            let data = b"lungo";
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            b.append_data(&mut header, "pkg/include/lungo.h", &data[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let good = hex(&Sha256::digest(&tarball));
        let target = dir.join("cache").join("x86_64-unknown-linux-gnu");
        let e = install_verified(&tarball, "test.tar.gz", &"0".repeat(64), &target).unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::RuntimeChecksum);
        assert!(!target.exists(), "nothing of a mismatching download is used");
        install_verified(&tarball, "test.tar.gz", &good, &target).unwrap();
        assert_eq!(fs::read(target.join("include/lungo.h")).unwrap(), b"lungo");
        assert_eq!(fs::read_to_string(target.join(VERIFIED)).unwrap().trim(), good);
        // A second install replaces the first.
        install_verified(&tarball, "test.tar.gz", &good, &target).unwrap();
        fs::remove_dir_all(&dir).unwrap();
    }
}
