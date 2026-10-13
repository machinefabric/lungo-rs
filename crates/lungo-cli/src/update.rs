//! `lungo self update`: this `lungo` replaced by the latest release of its channel.
//!
//! The release registry's manifest is trusted only through its signature ([`crate::release`]),
//! the archive only as the file the manifest describes (its size and SHA-256 digest), and the
//! command in it only once it runs and says it is the version it was published as. Then it
//! replaces the running one, where that one is.
//!
//! Two kinds of `lungo` are not replaced this way, and each is told what does update it:
//!
//! - one that is not a registry release — built from source, or by `cargo install lungo-cli` —
//!   has no release line, and so nothing it could trust;
//! - one a package manager installed is that package manager's: replacing its file would leave
//!   the package's record describing a file that is no longer there. A package says so with a
//!   file named `installed-by` beside the command, holding the package manager's name.

use crate::release::{self, Archive, Release, Source, Trust};
use lungo_build::{Error, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

/// The file a package leaves beside the command it installs, naming the package manager.
pub const INSTALLED_BY: &str = "installed-by";
/// Where an update downloads and unpacks, beside the command.
const STAGING: &str = ".lungo-update";

/// A self-update's outcome: the version before and the one now (equal when this `lungo` was the
/// latest release already).
#[derive(Debug, PartialEq, Eq)]
pub struct Updated {
    pub from: String,
    pub to: String,
}

/// `lungo self update`.
pub fn update() -> Result<Updated> {
    let identity = release::identity().ok_or_else(|| {
        Error::UpdateUnavailable(
            "this lungo is not a registry release, so it has no release line to update from: \
             `cargo install lungo-cli` installs the newest published one, as it installed this"
                .into(),
        )
    })?;
    let platform = release::platform().ok_or_else(|| {
        Error::UpdateUnavailable("the lungo command is not published for this platform: build it from source".into())
    })?;
    let running = std::env::current_exe().map_err(|e| Error::io("cannot tell where this lungo is", e))?;
    // Where the command is, not a link to it: a link on PATH into a package's directory must
    // find the package's `installed-by`, and replacing a link would leave the command behind it.
    let installed = lungo_build::canonical_path(&running)
        .map_err(|e| Error::io(format!("cannot resolve {}", running.display()), e))?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| Error::Environment("the clock is before 1970, so no certificate's validity can be checked".into()))?
        .as_secs();
    update_from(&Installation {
        source: &release::Https,
        registry: &identity.registry,
        channel: &identity.channel,
        trust: &identity.trust,
        now,
        installed: &installed,
        current: env!("CARGO_PKG_VERSION"),
        platform,
    })
}

/// Everything an update is decided by: what a build bakes in, the clock, and where the command
/// is.
pub struct Installation<'a> {
    pub source: &'a dyn Source,
    pub registry: &'a str,
    pub channel: &'a str,
    pub trust: &'a Trust,
    /// Seconds since 1970.
    pub now: u64,
    /// The command to replace: a file, not a link to one.
    pub installed: &'a Path,
    /// The version of the command at `installed`.
    pub current: &'a str,
    pub platform: &'a str,
}

/// [`update`] with everything it reads from the build and the machine passed explicitly.
pub fn update_from(at: &Installation) -> Result<Updated> {
    let directory = at.installed.parent().ok_or_else(|| {
        Error::UpdateUnavailable(format!("{} is not a file in a directory", at.installed.display()))
    })?;
    if let Some(manager) = package_manager(directory)? {
        return Err(Error::UpdateUnavailable(format!(
            "this lungo was installed by {manager}, which is what updates it: {}",
            upgrade_with(&manager)
        )));
    }
    let Some(release) =
        release::fetch_latest(at.source, at.registry, at.trust, at.channel, at.platform, at.now)?
    else {
        return Err(Error::UpdateUnavailable(format!(
            "no lungo release is published on the {} channel at {} yet",
            at.channel, at.registry
        )));
    };
    let latest = version(&release.version)
        .map_err(|why| Error::UpdateUntrusted(format!("the release manifest's latest version is {why}")))?;
    let current = version(at.current).map_err(|why| Error::Environment(format!("this lungo's version is {why}")))?;
    if latest <= current {
        return Ok(Updated { from: at.current.to_owned(), to: at.current.to_owned() });
    }
    // Beside the command, so that putting the new one in its place is a rename within one file
    // system: the command is the old one or the new one, never part of either.
    let staging = directory.join(STAGING);
    // One name, so that what an update that was stopped left behind is found by the next one
    // and cleared: a download or an unpacked archive that was never installed.
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| {
            Error::io(format!("cannot clear {}, which an earlier update left", staging.display()), e)
        })?;
    }
    fs::create_dir(&staging).map_err(|e| {
        Error::io(
            format!(
                "cannot write in {}, where this lungo is installed, so it cannot be replaced there \
                 (run the update with the rights that installed it)",
                directory.display()
            ),
            e,
        )
    })?;
    let outcome = install(at, &release, &staging);
    let cleared = fs::remove_dir_all(&staging);
    outcome?;
    cleared.map_err(|e| Error::io(format!("cannot remove {}", staging.display()), e))?;
    Ok(Updated { from: at.current.to_owned(), to: release.version })
}

/// Downloads `release` into `staging`, holds it to what the manifest says of it, and puts its
/// command in place of the installed one.
fn install(at: &Installation, release: &Release, staging: &Path) -> Result<()> {
    let Some(bytes) = at.source.get(&release.url)? else {
        return Err(Error::UpdateUnavailable(format!(
            "{} is listed by the release manifest and is not there",
            release.url
        )));
    };
    if bytes.len() as u64 != release.size {
        return Err(Error::UpdateUntrusted(format!(
            "{} is {} bytes; the release manifest says {}. The download was discarded",
            release.url,
            bytes.len(),
            release.size
        )));
    }
    let digest = crate::runtime::hex(&Sha256::digest(&bytes));
    if digest != release.sha256 {
        return Err(Error::UpdateUntrusted(format!(
            "{} has SHA-256 {digest}, but the release manifest lists {}. The download was discarded",
            release.url, release.sha256
        )));
    }
    let unpacked = match release.archive {
        Archive::TarGz => crate::runtime::unpack(&bytes, staging),
        Archive::Zip => unpack_zip(&bytes, staging),
    };
    unpacked.map_err(|e| Error::io(format!("cannot unpack {}", release.url), e))?;
    let name = format!("lungo{}", std::env::consts::EXE_SUFFIX);
    let new = staging.join(release::ARCHIVE_ROOT).join(&name);
    if !new.is_file() {
        return Err(Error::UpdateUntrusted(format!(
            "{} holds no {}/{name}, so it is not an archive of the lungo command",
            release.url,
            release::ARCHIVE_ROOT
        )));
    }
    // It must run, and be the release it was published as.
    let said = std::process::Command::new(&new).arg("--version").output().map_err(|e| {
        Error::UpdateUntrusted(format!("the lungo in {} does not run on this machine: {e}", release.url))
    })?;
    let said = String::from_utf8_lossy(&said.stdout).trim().to_owned();
    if said != format!("lungo {}", release.version) {
        return Err(Error::UpdateUntrusted(format!(
            "the lungo in {} says {said:?}, not \"lungo {}\"",
            release.url, release.version
        )));
    }
    replace(at.installed, &new)
}

/// The package manager that installed the command in `directory`, as its package recorded it.
fn package_manager(directory: &Path) -> Result<Option<String>> {
    let record = directory.join(INSTALLED_BY);
    match fs::read_to_string(&record) {
        Ok(text) => {
            let name = text.trim();
            if name.is_empty() || name.lines().count() != 1 {
                // A record that says nothing is still a package's: the command is not replaced
                // on a guess of whose it is.
                return Err(Error::UpdateUnavailable(format!(
                    "{} marks this lungo as a package's and does not name the package manager",
                    record.display()
                )));
            }
            Ok(Some(name.to_owned()))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(Error::io(format!("cannot read {}", record.display()), e)),
    }
}

/// What updates a `lungo` that `manager` installed.
fn upgrade_with(manager: &str) -> String {
    match manager {
        "Homebrew" => "`brew upgrade machinefabric/tap/lungo`".to_owned(),
        "apt" => "`sudo apt update && sudo apt install --only-upgrade lungo`".to_owned(),
        "dnf" => "`sudo dnf upgrade lungo`".to_owned(),
        other => format!("update the lungo package with {other}"),
    }
}

/// A release's version: three numbers. Anything else is said, not compared.
fn version(text: &str) -> std::result::Result<(u64, u64, u64), String> {
    let parts: Vec<&str> = text.split('.').collect();
    let number = |part: &str| -> Option<u64> {
        (!part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())).then(|| part.parse().ok()).flatten()
    };
    match parts[..] {
        [major, minor, patch] => match (number(major), number(minor), number(patch)) {
            (Some(major), Some(minor), Some(patch)) => Ok((major, minor, patch)),
            _ => Err(format!("{text:?}, which is not three numbers")),
        },
        _ => Err(format!("{text:?}, which is not three numbers")),
    }
}

fn unpack_zip(bytes: &[u8], into: &Path) -> std::io::Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(std::io::Error::other)?;
    // `extract` refuses entries that would be written outside the directory.
    archive.extract(into).map_err(std::io::Error::other)
}

/// Puts `new` where `installed` is.
#[cfg(not(windows))]
fn replace(installed: &Path, new: &Path) -> Result<()> {
    // One rename: a process that starts `lungo` meanwhile runs the old command or the new one.
    fs::rename(new, installed)
        .map_err(|e| Error::io(format!("cannot replace {}", installed.display()), e))
}

/// Puts `new` where `installed` is. Windows does not let a running program's file be replaced
/// or removed, and does let it be renamed: the running command is moved aside as `lungo.exe.old`
/// and the new one takes its name. The file moved aside stays until the next update removes it.
#[cfg(windows)]
fn replace(installed: &Path, new: &Path) -> Result<()> {
    let aside = aside(installed);
    match fs::remove_file(&aside) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(Error::io(format!("cannot remove {}", aside.display()), e)),
    }
    fs::rename(installed, &aside).map_err(|e| Error::io(format!("cannot move {} aside", installed.display()), e))?;
    if let Err(e) = fs::rename(new, installed) {
        // Back where it was: a failed update leaves the command that was there.
        return match fs::rename(&aside, installed) {
            Ok(()) => Err(Error::io(format!("cannot replace {}", installed.display()), e)),
            Err(back) => Err(Error::io(
                format!(
                    "cannot replace {} ({e}), and the command moved aside could not be put back: it is at {}",
                    installed.display(),
                    aside.display()
                ),
                back,
            )),
        };
    }
    Ok(())
}

/// Where the running command is moved to on Windows while the new one takes its place.
#[cfg(windows)]
fn aside(installed: &Path) -> PathBuf {
    let mut name = installed.file_name().expect("a command is a file").to_os_string();
    name.push(".old");
    installed.with_file_name(name)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::release::testing::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;
    use std::path::PathBuf;

    const REGISTRY: &str = "https://release.example";
    const PLATFORM: &str = "linux-x86_64";

    /// A registry a test serves from memory, recording what was asked of it.
    #[derive(Default)]
    struct Served {
        files: BTreeMap<String, Vec<u8>>,
        asked: RefCell<Vec<String>>,
    }

    impl Source for Served {
        fn get(&self, url: &str) -> Result<Option<Vec<u8>>> {
            self.asked.borrow_mut().push(url.to_owned());
            Ok(self.files.get(url).cloned())
        }
    }

    /// A directory of the test's own, removed when the test ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let dir = std::env::temp_dir().join(format!("lungo-update-test-{}-{name}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Scratch(lungo_build::canonical_path(&dir).unwrap())
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// A command that says it is `lungo <version>`.
    fn command(version: &str) -> Vec<u8> {
        format!("#!/bin/sh\necho 'lungo {version}'\n").into_bytes()
    }

    /// A release archive: `lungo/lungo` with `command` in it, and a licence.
    fn archive(root: &str, command: &[u8]) -> Vec<u8> {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default()));
        for (name, bytes, mode) in [("lungo", command, 0o755), ("LICENSE", &b"licence\n"[..], 0o644)] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(mode);
            header.set_cksum();
            tar.append_data(&mut header, format!("{root}/{name}"), bytes).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap()
    }

    fn installed(scratch: &Scratch, version: &str) -> PathBuf {
        let bin = scratch.0.join("bin");
        fs::create_dir_all(&bin).unwrap();
        let path = bin.join("lungo");
        fs::write(&path, command(version)).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A registry publishing `version` on the release channel as `archive`, signed by `line`.
    fn registry(line: &Line, version: &str, archive: &[u8]) -> Served {
        let url = format!("{REGISTRY}/lungo-cli/release/{version}/lungo-{version}-{PLATFORM}.tar.gz");
        let digest = crate::runtime::hex(&Sha256::digest(archive));
        let manifest = manifest("release", version, PLATFORM, &url, &digest, archive.len() as u64);
        let mut served = Served::default();
        served.files.insert(format!("{REGISTRY}/lungo-cli/manifest.sig"), line.sign(&manifest));
        served.files.insert(format!("{REGISTRY}/lungo-cli/manifest"), manifest);
        served.files.insert(url, archive.to_vec());
        served
    }

    fn run(served: &Served, line: &Line, installed: &Path, current: &str) -> Result<Updated> {
        let trust = line.trust("prod");
        update_from(&Installation {
            source: served,
            registry: REGISTRY,
            channel: "release",
            trust: &trust,
            now: NOW,
            installed,
            current,
            platform: PLATFORM,
        })
    }

    fn says(command: &Path) -> String {
        let out = std::process::Command::new(command).arg("--version").output().unwrap();
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// What is in the command's directory: the update leaves nothing of its own behind.
    fn beside(command: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(command.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    /// TEST0360: a newer release replaces the command where it is
    #[test]
    fn test0360_a_newer_release_replaces_the_command_where_it_is() {
        let scratch = Scratch::new("newer");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        // What an update that was stopped half way left beside the command.
        let left = lungo.parent().unwrap().join(STAGING).join("lungo");
        fs::create_dir_all(&left).unwrap();
        fs::write(left.join("lungo"), b"half a download").unwrap();
        let served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
        let updated = run(&served, &line, &lungo, "1.100.0").unwrap();
        assert_eq!(updated, Updated { from: "1.100.0".into(), to: "1.200.0".into() });
        assert_eq!(says(&lungo), "lungo 1.200.0");
        assert_eq!(beside(&lungo), ["lungo"], "nothing of the update is left beside the command");
    }

    /// TEST0361: the latest release is left as it is and nothing is downloaded
    #[test]
    fn test0361_the_latest_release_is_left_as_it_is_and_nothing_is_downloaded() {
        let scratch = Scratch::new("latest");
        let line = Line::new();
        for current in ["1.200.0", "1.200.1", "2.0.0"] {
            let lungo = installed(&scratch, current);
            let served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
            let updated = run(&served, &line, &lungo, current).unwrap();
            assert_eq!(updated, Updated { from: current.into(), to: current.into() });
            assert_eq!(says(&lungo), format!("lungo {current}"));
            assert_eq!(
                *served.asked.borrow(),
                [format!("{REGISTRY}/lungo-cli/manifest"), format!("{REGISTRY}/lungo-cli/manifest.sig")],
                "only the manifest and its signature were read"
            );
        }
    }

    /// TEST0362: versions are compared as numbers
    #[test]
    fn test0362_versions_are_compared_as_numbers() {
        // 1.99.0 < 1.100.0, which a comparison of text gets backwards.
        assert!(version("1.99.0").unwrap() < version("1.100.0").unwrap());
        assert!(version("1.100.9").unwrap() < version("1.100.10").unwrap());
        for refused in ["1.2", "1.2.3.4", "1.2.x", "1..3", "v1.2.3", "1.2.3-rc1", "1.2.+3", ""] {
            assert!(version(refused).is_err(), "{refused:?}");
        }
    }

    /// TEST0363: a manifest this lungo does not trust changes nothing
    #[test]
    fn test0363_a_manifest_this_lungo_does_not_trust_changes_nothing() {
        let scratch = Scratch::new("untrusted");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        let mut served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
        // Signed by a line whose roots this lungo was not built with.
        let other = Line { roots: [Key::new(4), Key::new(5), Key::new(6)], release: Key::new(9) };
        let manifest = served.files[&format!("{REGISTRY}/lungo-cli/manifest")].clone();
        served.files.insert(format!("{REGISTRY}/lungo-cli/manifest.sig"), other.sign(&manifest));
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUntrusted);
        assert!(e.to_string().contains("signed by 0 of this lungo's roots"), "{e}");
        assert_eq!(says(&lungo), "lungo 1.100.0");
        assert_eq!(served.asked.borrow().len(), 2, "no archive is fetched on the word of an untrusted manifest");

        // And one published with no signature at all.
        served.files.remove(&format!("{REGISTRY}/lungo-cli/manifest.sig"));
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUntrusted);
        assert!(e.to_string().contains("published without a signature"), "{e}");
    }

    /// TEST0364: an archive that is not the one the manifest describes is discarded
    #[test]
    fn test0364_an_archive_that_is_not_the_one_the_manifest_describes_is_discarded() {
        let scratch = Scratch::new("archive");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        let honest = archive("lungo", &command("1.200.0"));
        let url = format!("{REGISTRY}/lungo-cli/release/1.200.0/lungo-1.200.0-{PLATFORM}.tar.gz");

        // Another archive of the same size.
        let mut served = registry(&line, "1.200.0", &honest);
        let mut swapped = honest.clone();
        let last = swapped.len() - 1;
        swapped[last] ^= 1;
        served.files.insert(url.clone(), swapped);
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUntrusted);
        assert!(e.to_string().contains("has SHA-256"), "{e}");

        // One of another size.
        let mut served = registry(&line, "1.200.0", &honest);
        served.files.insert(url.clone(), honest[..honest.len() - 1].to_vec());
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert!(e.to_string().contains("bytes; the release manifest says"), "{e}");

        // One the manifest lists and the registry does not have.
        let mut served = registry(&line, "1.200.0", &honest);
        served.files.remove(&url);
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUnavailable);

        assert_eq!(says(&lungo), "lungo 1.100.0");
        assert_eq!(beside(&lungo), ["lungo"]);
    }

    /// TEST0365: a command that is not the version it was published as is not installed
    #[test]
    fn test0365_a_command_that_is_not_the_version_it_was_published_as_is_not_installed() {
        let scratch = Scratch::new("version");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        // Signed, intact, and holding another version's command.
        let served = registry(&line, "1.200.0", &archive("lungo", &command("1.150.0")));
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUntrusted);
        assert!(e.to_string().contains("says \"lungo 1.150.0\", not \"lungo 1.200.0\""), "{e}");

        // An archive of something else.
        let served = registry(&line, "1.200.0", &archive("other", &command("1.200.0")));
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert!(e.to_string().contains("holds no lungo/lungo"), "{e}");

        assert_eq!(says(&lungo), "lungo 1.100.0");
        assert_eq!(beside(&lungo), ["lungo"]);
    }

    /// TEST0366: a package managers lungo is left to the package manager
    #[test]
    fn test0366_a_package_managers_lungo_is_left_to_the_package_manager() {
        let scratch = Scratch::new("package");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        let served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
        let record = lungo.parent().unwrap().join(INSTALLED_BY);
        for (manager, command) in [
            ("Homebrew\n", "`brew upgrade machinefabric/tap/lungo`"),
            ("apt\n", "`sudo apt update && sudo apt install --only-upgrade lungo`"),
            ("dnf", "`sudo dnf upgrade lungo`"),
            ("pacman\n", "update the lungo package with pacman"),
        ] {
            fs::write(&record, manager).unwrap();
            let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
            assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUnavailable);
            let text = e.to_string();
            assert!(text.contains(&format!("installed by {}", manager.trim())) && text.contains(command), "{text}");
        }
        // A record that names nobody is still a package's.
        fs::write(&record, "\n").unwrap();
        let e = run(&served, &line, &lungo, "1.100.0").unwrap_err();
        assert!(e.to_string().contains("does not name the package manager"), "{e}");

        assert_eq!(says(&lungo), "lungo 1.100.0");
        assert!(served.asked.borrow().is_empty(), "the registry is not asked about a command that is not replaced");
    }

    /// TEST0367: a channel that has published nothing is said
    #[test]
    fn test0367_a_channel_that_has_published_nothing_is_said() {
        let scratch = Scratch::new("empty");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        let served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
        let trust = line.trust("prod");
        let e = update_from(&Installation {
            source: &served,
            registry: REGISTRY,
            channel: "nightly",
            trust: &trust,
            now: NOW,
            installed: &lungo,
            current: "1.100.0",
            platform: PLATFORM,
        })
        .unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::UpdateUnavailable);
        assert!(e.to_string().contains("no lungo release is published on the nightly channel"), "{e}");
        // And a registry with no manifest at all.
        let e = run(&Served::default(), &line, &lungo, "1.100.0").unwrap_err();
        assert!(e.to_string().contains("no lungo release is published at https://release.example yet"), "{e}");
    }

    /// TEST0368: a command this user cannot replace is said with where it is
    #[test]
    fn test0368_a_command_this_user_cannot_replace_is_said_with_where_it_is() {
        let scratch = Scratch::new("readonly");
        let line = Line::new();
        let lungo = installed(&scratch, "1.100.0");
        let bin = lungo.parent().unwrap().to_path_buf();
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o555)).unwrap();
        // Root writes where the mode says nobody may: the refusal is not there to see.
        let probe = bin.join(".probe");
        let root = fs::write(&probe, b"").is_ok();
        let _ = fs::remove_file(&probe);
        let served = registry(&line, "1.200.0", &archive("lungo", &command("1.200.0")));
        let outcome = run(&served, &line, &lungo, "1.100.0");
        fs::set_permissions(&bin, fs::Permissions::from_mode(0o755)).unwrap();
        if root {
            return;
        }
        let e = outcome.unwrap_err();
        assert_eq!(e.code(), lungo_build::ErrorCode::Io);
        let text = e.to_string();
        assert!(text.contains(&bin.display().to_string()) && text.contains("cannot be replaced there"), "{text}");
        assert_eq!(says(&lungo), "lungo 1.100.0");
    }
}
