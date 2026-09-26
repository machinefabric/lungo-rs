//! Resolution of the exact Lean toolchain a project pins.

use crate::error::{Error, Result};
use crate::fingerprint::Hasher;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Toolchains with a validated worker adapter.
pub const SUPPORTED_TOOLCHAINS: &[&str] = &["leanprover/lean4:v4.34.1"];

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
    /// The installation directory.
    pub root: PathBuf,
    pub lean: PathBuf,
    pub lake: PathBuf,
    /// `4.34.1`.
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
    if !SUPPORTED_TOOLCHAINS.contains(&pin.as_str()) {
        return Err(Error::UnsupportedToolchain {
            found: pin,
            supported: SUPPORTED_TOOLCHAINS.iter().map(|s| s.to_string()).collect(),
        });
    }
    Ok(pin)
}

fn elan_home() -> Result<PathBuf> {
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

/// The directory elan installs `pin` into.
fn elan_toolchain_dir(pin: &str) -> Result<PathBuf> {
    let dir = pin.replace('/', "--").replace(':', "---");
    Ok(elan_home()?.join("toolchains").join(dir))
}

/// Resolves the installation of `pin`, either from an explicit directory or from elan.
pub fn resolve(pin: &str, explicit_dir: Option<&Path>, policy: ToolchainPolicy) -> Result<Toolchain> {
    let root = match explicit_dir {
        Some(d) => d.to_path_buf(),
        None => elan_toolchain_dir(pin)?,
    };
    if !root.join("bin").join(exe("lean")).is_file() {
        match (explicit_dir, policy) {
            (None, ToolchainPolicy::Install) => {
                let status = Command::new(exe("elan"))
                    .args(["toolchain", "install", pin])
                    .status()
                    .map_err(|e| Error::io("cannot run elan", e))?;
                if !status.success() {
                    return Err(Error::Command {
                        program: format!("elan toolchain install {pin}"),
                        status: status.to_string(),
                        output: String::new(),
                    });
                }
            }
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
    let expected = pin.rsplit(":v").next().unwrap_or(pin);
    if version != expected {
        return Err(Error::Environment(format!(
            "the toolchain at {} is Lean {version}, but the project pins {pin}",
            root.display()
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
    Ok(Toolchain { pin: pin.to_owned(), root, lean, lake, version, githash, sysroot_identity: h.finish() })
}

impl Toolchain {
    /// Resolves the installation of the toolchain `pin` (see [`read_pin`]).
    pub fn resolve_pin(pin: &str, explicit_dir: Option<&Path>, policy: ToolchainPolicy) -> Result<Toolchain> {
        if !SUPPORTED_TOOLCHAINS.contains(&pin) {
            return Err(Error::UnsupportedToolchain {
                found: pin.to_owned(),
                supported: SUPPORTED_TOOLCHAINS.iter().map(|s| s.to_string()).collect(),
            });
        }
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

    #[test]
    fn parses_lean_version_banner() {
        let (v, c) = parse_version(
            "Lean (version 4.34.1, arm64-apple-darwin24.6.0, commit 5045d0056413266e57c625dcd7c365b10e377c52, Release)\n",
        )
        .unwrap();
        assert_eq!(v, "4.34.1");
        assert_eq!(c, "5045d0056413266e57c625dcd7c365b10e377c52");
        assert!(parse_version("lean 4").is_none());
    }

    #[test]
    fn floating_and_unknown_pins_are_rejected() {
        let dir = std::env::temp_dir().join(format!("ptn-pin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for pin in ["stable", "leanprover/lean4:stable", "leanprover/lean4:v4.35.0-rc3"] {
            std::fs::write(dir.join("lean-toolchain"), format!("{pin}\n")).unwrap();
            assert!(matches!(read_pin(&dir), Err(Error::UnsupportedToolchain { .. })), "{pin}");
        }
        std::fs::write(dir.join("lean-toolchain"), "leanprover/lean4:v4.34.1\n").unwrap();
        assert_eq!(read_pin(&dir).unwrap(), "leanprover/lean4:v4.34.1");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
