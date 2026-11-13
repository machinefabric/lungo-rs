//! elan, Lean's toolchain manager: `lungo setup` installs the elan release this lungo pins into
//! the per-user elan home when that home has none, and uses the one there otherwise. Every Lean
//! toolchain of the user is then installed by that one elan, into that one home.

use lungo_build::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The elan release `lungo setup` installs.
pub const ELAN_VERSION: &str = "4.2.4";

/// How an elan release archive is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Packing {
    TarGz,
    Zip,
}

/// The elan release archive for this host: its file name, its SHA-256 (as the elan release on
/// GitHub lists it) and how it is packed. Each holds only the installer, `elan-init`.
fn archive() -> Result<(&'static str, &'static str, Packing)> {
    Ok(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => (
            "elan-aarch64-apple-darwin.tar.gz",
            "7ad829861392c718dfebde3a83b5c8508df47be02af68894b094b0b3952616e5",
            Packing::TarGz,
        ),
        ("macos", "x86_64") => (
            "elan-x86_64-apple-darwin.tar.gz",
            "8a340b309d8ed2e96f930761fa223b3af57a38f5d253b53ac90293c9516f8cd4",
            Packing::TarGz,
        ),
        ("linux", "x86_64") => (
            "elan-x86_64-unknown-linux-gnu.tar.gz",
            "42b94d4244e8353142c456ec0e4ca6528fd898a6c604d4059f494e706e431f63",
            Packing::TarGz,
        ),
        ("linux", "aarch64") => (
            "elan-aarch64-unknown-linux-gnu.tar.gz",
            "05febd124d84ebf994b2e7479922a5650b1e950c17ae3bd1ddd776b65bb72bf9",
            Packing::TarGz,
        ),
        ("windows", "x86_64") => (
            "elan-x86_64-pc-windows-msvc.zip",
            "fad2e980a191c15884cc1d80d170ffc5fa84f3774541020145b66d1a644c6111",
            Packing::Zip,
        ),
        (os, arch) => {
            return Err(Error::ElanUnavailable(format!(
                "elan {ELAN_VERSION} has no release for {os}/{arch}, so lungo cannot install elan on this machine"
            )));
        }
    })
}

/// The pinned release of the elan home `home`, installed into it when it has no elan. Returns
/// whether it was installed.
pub fn ensure(home: &Path) -> Result<bool> {
    let (name, sha256, packing) = archive()?;
    let url = format!("https://github.com/leanprover/elan/releases/download/v{ELAN_VERSION}/{name}");
    ensure_with(home, &Pinned { source: &url, sha256, packing }, || crate::runtime::http_get(&url))
}

/// An elan release archive: where it comes from, its SHA-256 and how it is packed.
struct Pinned<'a> {
    source: &'a str,
    sha256: &'a str,
    packing: Packing,
}

/// [`ensure`], with the archive's bytes from `bytes` (or why they could not be had).
fn ensure_with(
    home: &Path,
    archive: &Pinned<'_>,
    bytes: impl FnOnce() -> std::result::Result<Vec<u8>, String>,
) -> Result<bool> {
    let elan = lungo_build::elan_binary(home);
    if elan.is_file() {
        return Ok(false);
    }
    let failed = |why: String| {
        Error::ElanUnavailable(format!(
            "cannot install elan {ELAN_VERSION} from {} into the elan home {}: {why}",
            archive.source,
            home.display()
        ))
    };
    fs::create_dir_all(home).map_err(|e| failed(format!("cannot create the elan home: {e}")))?;
    // Processes installing into one home take turns; one that finds elan installed meanwhile
    // uses it.
    let lock_path = home.join(".lungo-install.lock");
    let lock =
        fs::File::create(&lock_path).map_err(|e| failed(format!("cannot create {}: {e}", lock_path.display())))?;
    lock.lock().map_err(|e| failed(format!("cannot lock {}: {e}", lock_path.display())))?;
    if elan.is_file() {
        return Ok(false);
    }
    let bytes = bytes().map_err(|e| failed(format!("the download failed: {e}")))?;
    let digest = crate::runtime::hex(&Sha256::digest(&bytes));
    if digest != archive.sha256 {
        return Err(failed(format!(
            "the download has SHA-256 {digest}, but this lungo pins {}; it was discarded",
            archive.sha256
        )));
    }
    let staging = home.join(format!(".lungo-install-{}", std::process::id()));
    let installed = install(home, archive, &bytes, &staging).map_err(failed);
    let cleared = fs::remove_dir_all(&staging);
    installed?;
    cleared.map_err(|e| failed(format!("elan is installed, but {} cannot be removed: {e}", staging.display())))?;
    Ok(true)
}

/// Unpacks the verified archive `bytes` into `staging`, runs its installer into `home`, and
/// checks that the elan it installed is there and is the pinned release.
fn install(home: &Path, archive: &Pinned<'_>, bytes: &[u8], staging: &Path) -> std::result::Result<(), String> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|e| format!("cannot clear {}: {e}", staging.display()))?;
    }
    let unpacked = match archive.packing {
        Packing::TarGz => crate::runtime::unpack(bytes, staging),
        Packing::Zip => fs::create_dir_all(staging).and_then(|()| crate::update::unpack_zip(bytes, staging)),
    };
    unpacked.map_err(|e| format!("cannot unpack the archive: {e}"))?;
    let init: PathBuf = staging.join(format!("elan-init{}", std::env::consts::EXE_SUFFIX));
    if !init.is_file() {
        return Err(format!("the archive holds no {}", init.file_name().expect("a file name").to_string_lossy()));
    }
    let args = ["-y", "--no-modify-path", "--default-toolchain", "none"];
    let shown = format!("{} {}", init.display(), args.join(" "));
    let out = Command::new(&init)
        .args(args)
        .env("ELAN_HOME", home)
        .output()
        .map_err(|e| format!("cannot run `{shown}`: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "`{shown}` failed ({}):\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    let elan = lungo_build::elan_binary(home);
    if !elan.is_file() {
        return Err(format!("`{shown}` succeeded, but there is no {}", elan.display()));
    }
    let said = Command::new(&elan)
        .arg("--version")
        .env("ELAN_HOME", home)
        .output()
        .map_err(|e| format!("the installed {} does not run: {e}", elan.display()))?;
    let said = String::from_utf8_lossy(&said.stdout).trim().to_owned();
    if !said.starts_with(&format!("elan {ELAN_VERSION} ")) {
        return Err(format!("the installed {} says {said:?}, not elan {ELAN_VERSION}", elan.display()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An empty directory for one test.
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lungo-elan-{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).unwrap();
        }
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A `.tar.gz` holding `elan-init`, the shell script `script`.
    #[cfg(unix)]
    fn installer(script: &str) -> Vec<u8> {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        let mut header = tar::Header::new_gnu();
        header.set_size(script.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "elan-init", script.as_bytes()).unwrap();
        tar.into_inner().unwrap().finish().unwrap()
    }

    /// An installer that does what elan's does: puts an `elan` reporting `version` into
    /// `$ELAN_HOME/bin`, given the arguments lungo passes.
    #[cfg(unix)]
    fn elan_init(version: &str) -> String {
        format!(
            "#!/bin/sh\n\
             [ \"$*\" = \"-y --no-modify-path --default-toolchain none\" ] || {{ echo \"unexpected arguments: $*\" >&2; exit 2; }}\n\
             mkdir -p \"$ELAN_HOME/bin\"\n\
             printf '#!/bin/sh\\necho \"elan {version} (test)\"\\n' > \"$ELAN_HOME/bin/elan\"\n\
             chmod +x \"$ELAN_HOME/bin/elan\"\n"
        )
    }

    fn pinned<'a>(sha256: &'a str) -> Pinned<'a> {
        Pinned { source: "https://example.invalid/elan.tar.gz", sha256, packing: Packing::TarGz }
    }

    /// What the elan home holds, by name.
    fn listing(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        names
    }

    /// TEST0393: an elan home that has elan keeps it: nothing is downloaded or installed
    #[test]
    fn test0393_a_home_with_elan_keeps_it_and_nothing_is_downloaded() {
        let home = scratch("present");
        fs::create_dir_all(home.join("bin")).unwrap();
        fs::write(lungo_build::elan_binary(&home), b"the elan already there").unwrap();
        let installed = ensure_with(&home, &pinned(&"0".repeat(64)), || panic!("elan was downloaded")).unwrap();
        assert!(!installed);
        assert_eq!(listing(&home), ["bin"]);
        assert_eq!(fs::read(lungo_build::elan_binary(&home)).unwrap(), b"the elan already there");
        fs::remove_dir_all(&home).unwrap();
    }

    /// TEST0394: a download that is not the archive lungo pins is discarded, and the refusal
    /// names the download, the elan home and both digests
    #[test]
    fn test0394_a_download_that_is_not_the_pinned_archive_is_discarded() {
        let root = scratch("digest");
        let home = root.join(".elan");
        let expected = crate::runtime::hex(&Sha256::digest(b"the pinned archive"));
        let got = crate::runtime::hex(&Sha256::digest(b"another file"));
        let err = ensure_with(&home, &pinned(&expected), || Ok(b"another file".to_vec())).unwrap_err();
        assert_eq!(err.code(), lungo_build::ErrorCode::ElanUnavailable);
        let text = err.to_string();
        for part in ["https://example.invalid/elan.tar.gz", &home.display().to_string(), &expected, &got, "discarded"] {
            assert!(text.contains(part), "{part:?} is not in: {text}");
        }
        assert!(!lungo_build::elan_binary(&home).exists());
        assert_eq!(listing(&home), [".lungo-install.lock"]);
        fs::remove_dir_all(&root).unwrap();
    }

    /// TEST0395: the pinned archive's installer installs elan into the elan home, which is used
    /// from then on
    #[cfg(unix)]
    #[test]
    fn test0395_the_pinned_installer_installs_elan_into_the_home() {
        let root = scratch("install");
        let home = root.join(".elan");
        let archive = installer(&elan_init(ELAN_VERSION));
        let sha256 = crate::runtime::hex(&Sha256::digest(&archive));
        assert!(ensure_with(&home, &pinned(&sha256), || Ok(archive.clone())).unwrap());
        assert!(lungo_build::elan_binary(&home).is_file());
        assert_eq!(listing(&home), [".lungo-install.lock", "bin"], "the installer is not left behind");
        assert!(!ensure_with(&home, &pinned(&sha256), || panic!("elan was downloaded again")).unwrap());
        fs::remove_dir_all(&root).unwrap();
    }

    /// TEST0396: an installation that fails says what was attempted, from where, into which
    /// home, and why: a failed download, an installer that fails, and one that installs another
    /// elan
    #[cfg(unix)]
    #[test]
    fn test0396_a_failed_installation_says_what_from_where_and_why() {
        let root = scratch("fail");
        let home = root.join(".elan");
        let attempt = format!(
            "error[LNG0113]: cannot install elan {ELAN_VERSION} from https://example.invalid/elan.tar.gz into the elan home {}",
            home.display()
        );

        let err = ensure_with(&home, &pinned(&"0".repeat(64)), || Err("connection refused".into())).unwrap_err();
        assert_eq!(err.to_string(), format!("{attempt}: the download failed: connection refused"));

        let failing = installer("#!/bin/sh\necho 'no space left on device' >&2\nexit 3\n");
        let sha256 = crate::runtime::hex(&Sha256::digest(&failing));
        let text = ensure_with(&home, &pinned(&sha256), || Ok(failing.clone())).unwrap_err().to_string();
        assert!(text.starts_with(&attempt), "{text}");
        assert!(text.contains("elan-init -y --no-modify-path --default-toolchain none` failed"), "{text}");
        assert!(text.contains("exit status: 3"), "{text}");
        assert!(text.contains("no space left on device"), "{text}");
        assert!(!lungo_build::elan_binary(&home).exists());
        assert_eq!(listing(&home), [".lungo-install.lock"], "the installer is not left behind");

        let other = installer(&elan_init("1.0.0"));
        let sha256 = crate::runtime::hex(&Sha256::digest(&other));
        let text = ensure_with(&home, &pinned(&sha256), || Ok(other.clone())).unwrap_err().to_string();
        assert!(text.starts_with(&attempt), "{text}");
        assert!(text.contains(&format!("says \"elan 1.0.0 (test)\", not elan {ELAN_VERSION}")), "{text}");
        fs::remove_dir_all(&root).unwrap();
    }
}
