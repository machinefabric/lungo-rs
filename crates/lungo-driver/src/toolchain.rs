//! Resolution of the exact Lean toolchain a project pins.
//!
//! A project pins one Lean release (`leanprover/lean4:v<version>`) in `lean-toolchain`; lungo
//! builds it when the release is one it supports ([`LEAN_RELEASES`]) and refuses anything else —
//! another release, a release candidate, a nightly, or a channel such as `stable` — before Lake
//! runs.

use crate::error::{Error, Result};
use crate::fingerprint::Hasher;
use lungo_runtime::releases::{LEAN_RELEASES, LeanRelease, LeanVersion};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The `lean-toolchain` prefix of a Lean release's pin.
const PIN_PREFIX: &str = "leanprover/lean4:v";

/// The supported release `pin` names, or the error refusing it.
pub fn supported_release(pin: &str) -> Result<&'static LeanRelease> {
    let version = pin.strip_prefix(PIN_PREFIX).and_then(LeanVersion::parse);
    version.and_then(lungo_runtime::releases::release).ok_or_else(|| Error::UnsupportedToolchain {
        found: pin.to_owned(),
        supported: LEAN_RELEASES.iter().map(LeanRelease::pin).collect(),
        newer: version.is_some_and(|v| v > lungo_runtime::releases::newest().version),
    })
}

/// What to do when the pinned toolchain is not installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ToolchainPolicy {
    /// Fail: builds never download or install toolchains.
    #[default]
    Strict,
    /// Install the pinned toolchain with `elan`.
    Install,
}

#[derive(Debug, Clone)]
pub struct Toolchain {
    /// The pin, as written in `lean-toolchain`.
    pub pin: String,
    /// The supported release the pin names, which the installation is.
    pub release: &'static LeanRelease,
    /// The installation directory.
    pub root: PathBuf,
    pub lean: PathBuf,
    pub lake: PathBuf,
    /// `4.35.0`.
    pub version: String,
    pub githash: String,
    /// Identifies this installation: its location and the size and modification time of its
    /// compiler binaries and libraries.
    pub sysroot_identity: String,
}

fn exe(name: &str) -> String {
    format!("{name}{}", std::env::consts::EXE_SUFFIX)
}

/// Reads and validates the project's toolchain pin.
pub fn read_pin(project: &Path) -> Result<String> {
    let path = project.join("lean-toolchain");
    let text = std::fs::read_to_string(&path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
    let pin = text.trim().to_owned();
    if pin.is_empty() || pin.contains(char::is_whitespace) {
        return Err(Error::Project(format!("{} does not contain a single toolchain name", path.display())));
    }
    supported_release(&pin)?;
    Ok(pin)
}

/// The per-user elan home every Lean toolchain of the user is installed into: `ELAN_HOME`, else
/// `.elan` in the home directory.
pub fn elan_home() -> Result<PathBuf> {
    if let Some(h) = std::env::var_os("ELAN_HOME") {
        return Ok(PathBuf::from(h));
    }
    let home = std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).ok_or_else(|| {
        Error::Environment(
            "cannot locate the elan installation: neither ELAN_HOME nor the home directory is set".into(),
        )
    })?;
    Ok(PathBuf::from(home).join(".elan"))
}

/// The `elan` of the elan home `home`.
pub fn elan_binary(home: &Path) -> PathBuf {
    home.join("bin").join(exe("elan"))
}

/// The directory the elan of the elan home `home` installs `pin` into.
fn elan_toolchain_dir(home: &Path, pin: &str) -> PathBuf {
    home.join("toolchains").join(pin.replace('/', "--").replace(':', "---"))
}

/// Resolves the installation of `pin`, a supported release, either from an explicit directory or
/// from elan. The installation must be that release: its version and the commit it was built from.
pub fn resolve(pin: &str, explicit_dir: Option<&Path>, policy: ToolchainPolicy) -> Result<Toolchain> {
    let release = supported_release(pin)?;
    let (root, elan) = match explicit_dir {
        Some(d) => (d.to_path_buf(), None),
        None => {
            let home = elan_home()?;
            (elan_toolchain_dir(&home, pin), Some(home))
        }
    };
    if !root.join("bin").join(exe("lean")).is_file() {
        match (&elan, policy) {
            (Some(home), ToolchainPolicy::Install) => install_toolchain(home, pin)?,
            _ => {
                return Err(Error::ToolchainNotInstalled { toolchain: pin.to_owned(), expected_at: root });
            }
        }
    }
    let root = crate::canonical_path(&root).map_err(|e| Error::io(format!("cannot resolve {}", root.display()), e))?;
    let lean = root.join("bin").join(exe("lean"));
    let lake = root.join("bin").join(exe("lake"));
    let out = Command::new(&lean)
        .arg("--version")
        .output()
        .map_err(|e| Error::io(format!("cannot run {}", lean.display()), e))?;
    if !out.status.success() {
        return Err(Error::Command {
            program: format!("{} --version", lean.display()),
            status: out.status.to_string(),
            output: String::from_utf8_lossy(&out.stderr).into_owned(),
        });
    }
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let (version, githash) = parse_version(&text)
        .ok_or_else(|| Error::Environment(format!("unexpected `lean --version` output: {text:?}")))?;
    if version != release.version.to_string() {
        return Err(Error::Environment(format!(
            "the toolchain at {} is Lean {version}, but the project pins {pin}",
            root.display()
        )));
    }
    if githash != release.githash {
        return Err(Error::Environment(format!(
            "the toolchain at {} is Lean {version} built from commit {githash}, not the Lean {version} release (commit {})",
            root.display(),
            release.githash
        )));
    }
    let mut h = Hasher::new("sysroot");
    h.field(root.to_string_lossy().as_bytes());
    for rel in ["bin", "lib/lean", "lib"] {
        let dir = root.join(rel);
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        let mut names: Vec<_> = entries.filter_map(|e| e.ok()).map(|e| e.path()).collect();
        names.sort();
        for p in names {
            if let Ok(meta) = std::fs::metadata(&p)
                && meta.is_file()
            {
                h.field(p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default().as_bytes());
                h.field(&meta.len().to_le_bytes());
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                h.field(&mtime.to_le_bytes());
            }
        }
    }
    Ok(Toolchain { pin: pin.to_owned(), release, root, lean, lake, version, githash, sysroot_identity: h.finish() })
}

/// Installs `pin` with the elan of the elan home `home`, never an `elan` found on `PATH`.
fn install_toolchain(home: &Path, pin: &str) -> Result<()> {
    let elan = elan_binary(home);
    if !elan.is_file() {
        return Err(Error::ElanUnavailable(format!(
            "elan is not installed in the elan home {} (there is no {}); `lungo setup` installs it",
            home.display(),
            elan.display()
        )));
    }
    let status = Command::new(&elan)
        .args(["toolchain", "install", pin])
        .env("ELAN_HOME", home)
        .status()
        .map_err(|e| Error::io(format!("cannot run {}", elan.display()), e))?;
    if !status.success() {
        return Err(Error::Command {
            program: format!("{} toolchain install {pin}", elan.display()),
            status: status.to_string(),
            output: String::new(),
        });
    }
    Ok(())
}

impl Toolchain {
    /// Resolves the installation of the toolchain `pin` (see [`read_pin`]).
    pub fn resolve_pin(pin: &str, explicit_dir: Option<&Path>, policy: ToolchainPolicy) -> Result<Toolchain> {
        resolve(pin, explicit_dir, policy)
    }
}

/// Parses `Lean (version 4.34.1, <triple>, commit <hash>, Release)`.
fn parse_version(text: &str) -> Option<(String, String)> {
    let inner = text.trim().strip_prefix("Lean (version ")?;
    let version = inner.split(',').next()?.trim().to_owned();
    let commit = inner.split("commit ").nth(1)?.split([',', ')']).next()?.trim().to_owned();
    Some((version, commit))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0140: parses lean version banner
    #[test]
    fn test0140_parses_lean_version_banner() {
        let (v, c) = parse_version(
            "Lean (version 4.34.1, arm64-apple-darwin24.6.0, commit 5045d0056413266e57c625dcd7c365b10e377c52, Release)\n",
        )
        .unwrap();
        assert_eq!(v, "4.34.1");
        assert_eq!(c, "5045d0056413266e57c625dcd7c365b10e377c52");
        assert!(parse_version("lean 4").is_none());
    }

    /// An empty directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lungo-toolchain-{name}-{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// TEST0397: a missing toolchain is installed by the elan of the elan home lungo resolved,
    /// told that home, never by an `elan` on PATH
    #[cfg(unix)]
    #[test]
    fn test0397_the_toolchain_is_installed_by_the_elan_of_the_resolved_home() {
        use std::os::unix::fs::PermissionsExt;
        let home = scratch("install");
        let pin = "leanprover/lean4:v4.35.0";
        std::fs::create_dir_all(home.join("bin")).unwrap();
        let elan = elan_binary(&home);
        std::fs::write(
            &elan,
            "#!/bin/sh\nprintf '%s|%s' \"$ELAN_HOME\" \"$*\" > \"$ELAN_HOME/called\"\n\
             mkdir -p \"$ELAN_HOME/toolchains/leanprover--lean4---v4.35.0/bin\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&elan, std::fs::Permissions::from_mode(0o755)).unwrap();
        install_toolchain(&home, pin).unwrap();
        let called = std::fs::read_to_string(home.join("called")).unwrap();
        assert_eq!(called, format!("{}|toolchain install {pin}", home.display()));
        assert!(elan_toolchain_dir(&home, pin).join("bin").is_dir());
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// TEST0398: an elan home without elan is refused, naming the home and the elan it lacks
    #[test]
    fn test0398_an_elan_home_without_elan_is_refused_naming_it() {
        let home = scratch("missing");
        let err = install_toolchain(&home, "leanprover/lean4:v4.35.0").unwrap_err();
        assert_eq!(err.code(), crate::ErrorCode::ElanUnavailable);
        assert_eq!(
            err.to_string(),
            format!(
                "error[LNG0113]: elan is not installed in the elan home {} (there is no {}); `lungo setup` installs it",
                home.display(),
                elan_binary(&home).display()
            )
        );
        std::fs::remove_dir_all(&home).unwrap();
    }

    /// TEST0141: every supported release is accepted; floating, prerelease and unknown pins are
    /// rejected, and a release newer than lungo knows is told apart
    #[test]
    fn test0141_supported_releases_are_accepted_and_other_pins_rejected() {
        let dir = std::env::temp_dir().join(format!("lng-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for pin in ["leanprover/lean4:v4.34.1", "leanprover/lean4:v4.35.0"] {
            std::fs::write(dir.join("lean-toolchain"), format!("{pin}\n")).unwrap();
            assert_eq!(read_pin(&dir).unwrap(), pin);
        }
        let rejected = [
            ("stable", false),
            ("leanprover/lean4:stable", false),
            ("leanprover/lean4:v4.35.0-rc3", false),
            ("leanprover/lean4:nightly-2026-10-01", false),
            ("leanprover/lean4:v4.34.0", false),
            ("leanprover/lean4:v4.33.0", false),
            ("leanprover/lean4:4.35.0", false),
            ("leanprover/lean4:v4.99.0", true),
            ("leanprover/lean4:v5.0.0", true),
        ];
        for (pin, newer_release) in rejected {
            std::fs::write(dir.join("lean-toolchain"), format!("{pin}\n")).unwrap();
            match read_pin(&dir) {
                Err(e @ Error::UnsupportedToolchain { newer, .. }) => {
                    assert_eq!(newer, newer_release, "{pin}");
                    let text = e.to_string();
                    assert!(text.contains("leanprover/lean4:v4.34.1, leanprover/lean4:v4.35.0"), "{text}");
                    assert_eq!(text.contains("newer lungo may support it"), newer_release, "{text}");
                }
                other => panic!("{pin}: {other:?}"),
            }
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
