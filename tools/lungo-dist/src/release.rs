//! What a lungo release is made of, and how each of its artifacts is built and named.
//!
//! A release publishes, for one version:
//!
//! - the runtime archive of every target of [`RUNTIME_TARGETS`], and the XCFramework of the Apple
//!   runtimes, in the release registry under `lungo-runtime`;
//! - the `lungo` command of every platform of [`CLI_PLATFORMS`], built with the runtime manifest
//!   of those archives embedded ([`manifest`]), under `lungo`;
//! - `lungo-py`'s wheels ([`WHEELS`]), `lungo-ts`, the crates, and the distribution
//!   repositories `lungo-go` and `lungo-swift`, assembled from the published runtime.
//!
//! Each command here builds one artifact. Publishing runs them, uploads what they write, and
//! records it in the release manifest it signs; [`manifest`], [`go`] and [`swift`] read back
//! only what was published (verified against the digests the manifest lists), so what a
//! consumer downloads is exactly what the release recorded.

use crate::{Linker, Result, VERSION, archive, host, io, repo, run, runtime, sha256_file, support};
use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Every target a release has a runtime archive for.
pub const RUNTIME_TARGETS: &[&str] = &[
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "aarch64-apple-darwin",
    "x86_64-apple-darwin",
    "aarch64-apple-ios",
    "aarch64-apple-ios-sim",
    "x86_64-apple-ios",
    "x86_64-pc-windows-msvc",
    "x86_64-pc-windows-gnu",
    "wasm32-wasip1",
];

/// The release registry platform of the XCFramework, and its key in the runtime manifest.
pub const XCFRAMEWORK_PLATFORM: &str = "apple-xcframework";
pub const XCFRAMEWORK_KEY: &str = "xcframework";

/// The platforms of the `lungo` command (the workspace's `{os}-{arch}` names) and their targets.
/// The Linux builds are static (musl): one binary runs on every distribution. The Windows build
/// is linked by zig for the GNU ABI, like the Linux ones, so that the Mac a release is published
/// from builds every platform's command; the command links no runtime a generated package uses.
pub const CLI_PLATFORMS: &[(&str, &str)] = &[
    ("darwin-arm64", "aarch64-apple-darwin"),
    ("darwin-x86_64", "x86_64-apple-darwin"),
    ("linux-x86_64", "x86_64-unknown-linux-musl"),
    ("linux-arm64", "aarch64-unknown-linux-musl"),
    ("windows-x86_64", "x86_64-pc-windows-gnu"),
];

/// `lungo-py`'s wheels: the target whose shared runtime each carries, and its platform tag.
pub const WHEELS: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "manylinux_2_28_x86_64"),
    ("aarch64-unknown-linux-gnu", "manylinux_2_28_aarch64"),
    ("aarch64-apple-darwin", "macosx_12_0_arm64"),
    ("x86_64-apple-darwin", "macosx_12_0_x86_64"),
    ("x86_64-pc-windows-msvc", "win_amd64"),
];

/// The glibc the Linux GNU runtimes are linked against: the manylinux baseline of the wheels.
pub const GLIBC: &str = "2.28";

/// The zig `cargo zigbuild` links with, pinned: another version links differently.
pub const ZIG_VERSION: &str = "0.15.2";

pub fn runtime_archive_name(target: &str) -> String {
    format!("lungo-runtime-{VERSION}-{target}.tar.gz")
}

pub fn xcframework_name() -> String {
    format!("LungoRuntime-{VERSION}.xcframework.zip")
}

pub fn cli_archive_name(platform: &str) -> String {
    let ext = if platform.starts_with("windows-") { "zip" } else { "tar.gz" };
    format!("lungo-{VERSION}-{platform}.{ext}")
}

/// The targets the Go module carries a runtime for.
fn go_targets() -> Vec<&'static str> {
    RUNTIME_TARGETS.iter().copied().filter(|t| support::go_platform(t).is_some()).collect()
}

/// What a release is made of, as JSON, with every artifact's name.
pub fn targets() -> Result<()> {
    let json = serde_json::json!({
        "version": VERSION,
        "runtime": RUNTIME_TARGETS.iter().map(|t| (t.to_string(), runtime_archive_name(t))).collect::<BTreeMap<_, _>>(),
        "xcframework": { "platform": XCFRAMEWORK_PLATFORM, "name": xcframework_name() },
        "cli": CLI_PLATFORMS.iter().map(|(p, t)| (p.to_string(), serde_json::json!({ "target": t, "name": cli_archive_name(p) }))).collect::<BTreeMap<_, _>>(),
        "wheels": WHEELS.iter().map(|(t, tag)| (t.to_string(), tag.to_string())).collect::<BTreeMap<_, _>>(),
        "go": go_targets(),
    });
    println!("{}", serde_json::to_string_pretty(&json).expect("JSON"));
    Ok(())
}

/// Checks that `cargo zigbuild` and the pinned zig are installed, and for a Windows GNU `target`
/// MinGW-w64's `dlltool`, which Rust runs for the raw-dylib imports of the Windows API bindings.
pub fn check_zig(target: &str) -> Result<()> {
    let zig = Command::new("zig").arg("version").output().map_err(|e| {
        format!("zig {ZIG_VERSION} links the Linux and Windows GNU runtimes, and is not installed ({e})")
    })?;
    let found = String::from_utf8_lossy(&zig.stdout).trim().to_owned();
    if found != ZIG_VERSION {
        return Err(format!("the runtimes are linked with zig {ZIG_VERSION}, and the zig on PATH is {found}"));
    }
    // The binary, not the cargo subcommand: `cargo zigbuild` passes its arguments to the build,
    // and since 0.23 refuses `--version` there.
    let zigbuild = Command::new("cargo-zigbuild").arg("--version").output();
    if !zigbuild.is_ok_and(|o| o.status.success()) {
        return Err("`cargo zigbuild` is not installed: `cargo install --locked cargo-zigbuild`".into());
    }
    if let Some(arch) = target.strip_suffix("-pc-windows-gnu") {
        let dlltool = format!("{arch}-w64-mingw32-dlltool");
        if !Command::new(&dlltool).arg("--version").output().is_ok_and(|o| o.status.success()) {
            return Err(format!(
                "{target} is linked with MinGW-w64's `{dlltool}` for its Windows imports, and it is not on PATH \
                 (MinGW-w64: `brew install mingw-w64`, or your system's package)"
            ));
        }
    }
    Ok(())
}

/// How `target` is linked for a release, on this host: the Apple targets need a Mac and the
/// MSVC target Windows (their toolchains exist nowhere else); the Linux and Windows GNU targets
/// are linked by zig from any host; WebAssembly by Rust's own linker.
fn linker_for(target: &str) -> Result<Linker> {
    let host = host();
    if target.contains("-apple-") {
        return if host.contains("-apple-darwin") {
            Ok(Linker::Host)
        } else {
            Err(format!("{target} is built on a Mac (its SDK exists nowhere else); this is {host}"))
        };
    }
    if target.ends_with("-windows-msvc") {
        return if host.ends_with("-windows-msvc") {
            Ok(Linker::Host)
        } else {
            Err(format!("{target} is built on Windows (MSVC exists nowhere else); this is {host}"))
        };
    }
    if target.starts_with("wasm32-") {
        return Ok(Linker::Host);
    }
    if target.contains("-linux-") || target.ends_with("-windows-gnu") {
        return Ok(Linker::Zig);
    }
    Err(format!("{target} is not a target lungo releases"))
}

/// A fresh scratch directory for building `what`.
fn work_dir(what: &str) -> Result<PathBuf> {
    let dir = crate::target_dir().join("lungo-dist").join("release-work").join(what);
    if dir.exists() {
        io(format!("cannot clear {}", dir.display()), fs::remove_dir_all(&dir))?;
    }
    io(format!("cannot create {}", dir.display()), fs::create_dir_all(&dir))?;
    Ok(dir)
}

fn create_out(out_dir: &Path) -> Result<()> {
    io(format!("cannot create {}", out_dir.display()), fs::create_dir_all(out_dir))
}

/// The runtime archive of `target` in `out_dir`.
pub fn runtime_archive(target: &str, out_dir: &Path) -> Result<()> {
    if !RUNTIME_TARGETS.contains(&target) {
        return Err(format!("{target} is not a release target (targets: {})", RUNTIME_TARGETS.join(", ")));
    }
    let linker = linker_for(target)?;
    let work = work_dir(&format!("runtime-{target}"))?;
    let package = work.join(format!("lungo-runtime-{VERSION}-{target}"));
    runtime(target, &package, linker)?;
    create_out(out_dir)?;
    let out = out_dir.join(runtime_archive_name(target));
    archive(&package, &out)?;
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    Ok(())
}

/// The XCFramework of the Apple runtimes (macOS and the iOS simulator universal, iOS) in
/// `out_dir`, zipped as SwiftPM downloads it; prints its SwiftPM checksum (its SHA-256).
pub fn xcframework(out_dir: &Path) -> Result<()> {
    let work = work_dir("xcframework")?;
    let mut libs = BTreeMap::new();
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "aarch64-apple-ios",
        "aarch64-apple-ios-sim",
        "x86_64-apple-ios",
    ] {
        let linker = linker_for(target)?;
        let package = work.join(target);
        runtime(target, &package, linker)?;
        libs.insert(target, package.join("lib/liblungo.a"));
    }
    let universal = |name: &str, a: &str, b: &str| -> Result<PathBuf> {
        let dir = work.join("universal").join(name);
        io("cannot create the universal libraries", fs::create_dir_all(&dir))?;
        let out = dir.join("liblungo.a");
        run(Command::new("lipo").arg("-create").arg(&libs[a]).arg(&libs[b]).arg("-output").arg(&out))?;
        Ok(out)
    };
    let macos = universal("macos", "aarch64-apple-darwin", "x86_64-apple-darwin")?;
    let simulator = universal("ios-sim", "aarch64-apple-ios-sim", "x86_64-apple-ios")?;
    let framework = work.join("LungoRuntime.xcframework");
    support::xcframework(&[macos, libs["aarch64-apple-ios"].clone(), simulator], &framework)?;
    create_out(out_dir)?;
    let out = out_dir.join(xcframework_name());
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_file(&out))?;
    }
    run(Command::new("ditto").args(["-c", "-k", "--keepParent"]).arg(&framework).arg(&out))?;
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    println!("checksum {}", sha256_file(&out)?);
    Ok(())
}

/// One artifact of a published release: where it is and its SHA-256.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Published {
    url: String,
    sha256: String,
}

/// The artifacts of this version on `channel` of the release manifest `text` (release registry
/// schema 5.0), by platform: the installer of each platform named `name(platform)`.
fn published(
    text: &str,
    channel: &str,
    platforms: &[String],
    name: &dyn Fn(&str) -> String,
) -> Result<BTreeMap<String, Published>> {
    let m: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("the release manifest is not JSON: {e}"))?;
    if m["schemaVersion"] != "5.0" {
        return Err(format!("the release manifest has schema {}, not 5.0", m["schemaVersion"]));
    }
    let version = m["channels"][channel]["versions"]
        .get(VERSION)
        .ok_or_else(|| format!("the release manifest has no version {VERSION} on channel {channel}"))?;
    let mut out = BTreeMap::new();
    let mut missing = Vec::new();
    for p in platforms {
        let wanted = name(p);
        let installer = version["platforms"][p.as_str()]["installers"]
            .as_array()
            .and_then(|list| list.iter().find(|i| i["name"] == wanted.as_str()));
        match installer {
            Some(i) => {
                let url = i["path"].as_str().ok_or_else(|| format!("{wanted} has no path"))?.to_owned();
                let sha256 = i["sha256"].as_str().ok_or_else(|| format!("{wanted} has no sha256"))?.to_owned();
                if !url.starts_with("https://") {
                    return Err(format!("{wanted} is not served over HTTPS ({url})"));
                }
                if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
                    return Err(format!("{wanted} has no SHA-256 digest"));
                }
                out.insert(p.clone(), Published { url, sha256 });
            }
            None => missing.push(format!("{p} ({wanted})")),
        }
    }
    if !missing.is_empty() {
        return Err(format!(
            "lungo {VERSION} on {channel} is not complete: publish the runtime of {} first",
            missing.join(", ")
        ));
    }
    Ok(out)
}

/// The runtime manifest the `lungo` command embeds, from the release manifest of the runtime as
/// published (whose signature publishing verified before handing it here). Every target and the
/// XCFramework must be published: a command released with a partial manifest would never know
/// the rest.
pub fn manifest(release_manifest: &Path, channel: &str, out: &Path) -> Result<()> {
    let text = io(format!("cannot read {}", release_manifest.display()), fs::read_to_string(release_manifest))?;
    let manifest = runtime_manifest(&text, channel)?;
    io(
        format!("cannot write {}", out.display()),
        fs::write(out, format!("{}\n", serde_json::to_string_pretty(&manifest).expect("JSON"))),
    )
}

fn runtime_manifest(text: &str, channel: &str) -> Result<serde_json::Value> {
    let mut platforms: Vec<String> = RUNTIME_TARGETS.iter().map(|t| t.to_string()).collect();
    platforms.push(XCFRAMEWORK_PLATFORM.to_owned());
    let name = |p: &str| if p == XCFRAMEWORK_PLATFORM { xcframework_name() } else { runtime_archive_name(p) };
    let artifacts: BTreeMap<String, serde_json::Value> = published(text, channel, &platforms, &name)?
        .into_iter()
        .map(|(p, a)| {
            let key = if p == XCFRAMEWORK_PLATFORM { XCFRAMEWORK_KEY.to_owned() } else { p };
            (key, serde_json::json!({ "url": a.url, "sha256": a.sha256 }))
        })
        .collect();
    Ok(serde_json::json!({ "version": VERSION, "abi_version": lungo_runtime::ABI_VERSION, "artifacts": artifacts }))
}

/// A runtime manifest written by [`manifest`]: its artifacts by key.
fn read_manifest(path: &Path) -> Result<BTreeMap<String, Published>> {
    let text = io(format!("cannot read {}", path.display()), fs::read_to_string(path))?;
    let m: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{} is not JSON: {e}", path.display()))?;
    if m["version"] != VERSION {
        return Err(format!("{} is the runtime manifest of lungo {}, not {VERSION}", path.display(), m["version"]));
    }
    let artifacts = m["artifacts"].as_object().ok_or_else(|| format!("{} lists no artifacts", path.display()))?;
    artifacts
        .iter()
        .map(|(k, a)| {
            let field = |f: &str| a[f].as_str().map(str::to_owned).ok_or_else(|| format!("{k} has no {f}"));
            Ok((k.clone(), Published { url: field("url")?, sha256: field("sha256")? }))
        })
        .collect()
}

/// Downloads `a`, checks its SHA-256, and returns the bytes.
fn download(a: &Published) -> Result<Vec<u8>> {
    let mut response = ureq::get(&a.url).call().map_err(|e| format!("cannot download {}: {e}", a.url))?;
    let mut bytes = Vec::new();
    response.body_mut().as_reader().read_to_end(&mut bytes).map_err(|e| format!("cannot download {}: {e}", a.url))?;
    let digest: String = sha2::Digest::finalize(<sha2::Sha256 as sha2::Digest>::new_with_prefix(&bytes))
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    if digest != a.sha256 {
        return Err(format!(
            "{} has SHA-256 {digest}, but the release lists {}; nothing of it is used",
            a.url, a.sha256
        ));
    }
    Ok(bytes)
}

/// Downloads, checks and unpacks the runtime archive of `target` into `into`, returning the
/// runtime package (the archive's single directory).
fn published_runtime(artifacts: &BTreeMap<String, Published>, target: &str, into: &Path) -> Result<PathBuf> {
    let a = artifacts.get(target).ok_or_else(|| format!("the runtime manifest has no runtime for {target}"))?;
    let bytes = download(a)?;
    let dir = into.join(target);
    io("cannot create the runtime directory", fs::create_dir_all(&dir))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(bytes.as_slice()));
    for entry in io(format!("cannot read {}", a.url), archive.entries())? {
        let mut entry = io(format!("cannot read {}", a.url), entry)?;
        if !io(format!("cannot unpack {}", a.url), entry.unpack_in(&dir))? {
            return Err(format!("{} has an entry outside its directory", a.url));
        }
    }
    let root = dir.join(format!("lungo-runtime-{VERSION}-{target}"));
    if !root.join("include/lungo.h").is_file() {
        return Err(format!("{} does not hold the runtime package lungo-runtime-{VERSION}-{target}", a.url));
    }
    Ok(root)
}

/// The Go module `lungo-go` in `out` (replaced), with the published runtimes of its targets.
pub fn go(manifest: &Path, out: &Path) -> Result<()> {
    let artifacts = read_manifest(manifest)?;
    let work = work_dir("go")?;
    let mut runtimes = Vec::new();
    for target in go_targets() {
        runtimes.push((target.to_owned(), published_runtime(&artifacts, target, &work)?));
    }
    support::go(&runtimes, out)?;
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    Ok(())
}

/// The Swift package `lungo-swift` in `out` (replaced), whose runtime is the published
/// XCFramework (SwiftPM's checksum is its SHA-256).
pub fn swift(manifest: &Path, out: &Path) -> Result<()> {
    let artifacts = read_manifest(manifest)?;
    let framework =
        artifacts.get(XCFRAMEWORK_KEY).ok_or_else(|| "the runtime manifest has no XCFramework".to_owned())?;
    let work = work_dir("swift")?;
    // The native libraries a Swift binary links come from the macOS runtime package.
    let macos = published_runtime(&artifacts, "aarch64-apple-darwin", &work)?;
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    io(format!("cannot create {}", out.display()), fs::create_dir_all(out))?;
    let target = format!(
        ".binaryTarget(name: \"LungoRuntime\", url: {}, checksum: {})",
        serde_json::to_string(&framework.url).expect("JSON"),
        serde_json::to_string(&framework.sha256).expect("JSON")
    );
    support::swift(&target, &macos, out)?;
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    Ok(())
}

/// The `lungo` command of `platform`, with the runtime manifest `manifest` embedded, archived in
/// `out_dir` (as `lungo/`) with its licence and README.
pub fn cli(platform: &str, manifest: &Path, out_dir: &Path) -> Result<()> {
    let target = CLI_PLATFORMS.iter().find(|(p, _)| *p == platform).map(|(_, t)| *t).ok_or_else(|| {
        let known: Vec<&str> = CLI_PLATFORMS.iter().map(|(p, _)| *p).collect();
        format!("{platform} is not a platform of the lungo command (platforms: {})", known.join(", "))
    })?;
    // The command refuses to build with a manifest of another version; checked here first so the
    // error names the file rather than a build script.
    read_manifest(manifest)?;
    let manifest = io("cannot resolve the manifest", dunce::canonicalize(manifest))?;
    let linker = linker_for(target)?;
    let target_dir = crate::target_dir().join("lungo-dist").join("cli");
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo())
        .env("CARGO_TARGET_DIR", &target_dir)
        .env("LUNGO_RUNTIME_MANIFEST", &manifest)
        .env("MACOSX_DEPLOYMENT_TARGET", lungo_runtime::header::MACOS_DEPLOYMENT_TARGET)
        .env("CARGO_ENCODED_RUSTFLAGS", crate::remapped_paths().join("\u{1f}"));
    match linker {
        Linker::Host => cmd.args(["build", "--release", "-p", "lungo-cli", "--target", target]),
        Linker::Zig => {
            check_zig(target)?;
            cmd.args(["zigbuild", "--release", "-p", "lungo-cli", "--target", target])
        }
    };
    run(&mut cmd)?;
    let exe = if target.contains("-windows-") { "lungo.exe" } else { "lungo" };
    let work = work_dir(&format!("cli-{platform}"))?;
    // A fixed directory, which the OS packages are made from: the version and platform are in
    // the archive's name.
    let package = work.join("lungo");
    io("cannot create the package", fs::create_dir_all(&package))?;
    let repo = repo();
    for (from, to) in [
        (target_dir.join(target).join("release").join(exe), package.join(exe)),
        (repo.join("LICENSE"), package.join("LICENSE")),
        (repo.join("crates/lungo-cli/README.md"), package.join("README.md")),
    ] {
        io(format!("cannot copy {}", from.display()), fs::copy(&from, &to).map(|_| ()))?;
    }
    create_out(out_dir)?;
    let out = out_dir.join(cli_archive_name(platform));
    if platform.starts_with("windows-") {
        zip_dir(&package, &out)?;
    } else {
        archive(&package, &out)?;
    }
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    Ok(())
}

/// A zip of `dir`, whose entries are under its name, with fixed times.
fn zip_dir(dir: &Path, out: &Path) -> Result<()> {
    let name = dir.file_name().ok_or("the directory has no name")?.to_string_lossy().into_owned();
    let file = io(format!("cannot create {}", out.display()), fs::File::create(out))?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default());
    let mut entries: Vec<PathBuf> = io("cannot read the package", fs::read_dir(dir))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()
        .map_err(|e| e.to_string())?;
    entries.sort();
    for path in entries {
        let file_name = path.file_name().expect("entries have names").to_string_lossy().into_owned();
        zip.start_file(format!("{name}/{file_name}"), options)
            .map_err(|e| format!("cannot write {}: {e}", out.display()))?;
        let bytes = io(format!("cannot read {}", path.display()), fs::read(&path))?;
        io(format!("cannot write {}", out.display()), zip.write_all(&bytes))?;
    }
    zip.finish().map_err(|e| format!("cannot finish {}: {e}", out.display()))?;
    Ok(())
}

/// The platform tag of the wheel a release publishes for `target`.
fn release_wheel_tag(target: &str) -> Result<&'static str> {
    WHEELS.iter().find(|(t, _)| *t == target).map(|(_, tag)| *tag).ok_or_else(|| {
        let known: Vec<&str> = WHEELS.iter().map(|(t, _)| *t).collect();
        format!("lungo-py has no wheel for {target} (wheels: {})", known.join(", "))
    })
}

/// The platform tag of a wheel built on this machine for this machine alone.
///
/// A Linux one is linked by the host's toolchain against the host's glibc, so it is tagged for
/// this Linux and no other (`linux_x86_64`): the `manylinux` tag is a claim about every system
/// at that glibc, which only the runtime zig links against [`GLIBC`] can make. Elsewhere the
/// host's toolchain is what a release links with too, and the tag is the release's.
fn local_wheel_tag(target: &str) -> Result<String> {
    let release = release_wheel_tag(target)?;
    Ok(match target.strip_suffix("-unknown-linux-gnu") {
        Some(arch) => format!("linux_{arch}"),
        None => release.to_owned(),
    })
}

/// `lungo-py`'s wheel of `target` in `out_dir`: the package with the target's shared runtime,
/// tagged for the target's platform (the wheel holds no extension, so one serves every Python 3).
pub fn wheel(target: &str, python: &Path, out_dir: &Path) -> Result<()> {
    build_wheel(target, release_wheel_tag(target)?, linker_for(target)?, python, out_dir)
}

/// Builds `lungo-py`'s wheel for this machine into `out_dir`: the runtime linked by the host's
/// toolchain, as the rest of a local distribution is, and tagged for this machine. A local
/// distribution needs none of what a release links with.
pub fn local_wheel(target: &str, python: &Path, out_dir: &Path) -> Result<()> {
    build_wheel(target, &local_wheel_tag(target)?, Linker::Host, python, out_dir)
}

fn build_wheel(target: &str, tag: &str, linker: Linker, python: &Path, out_dir: &Path) -> Result<()> {
    let work = work_dir(&format!("wheel-{target}"))?;
    let package = work.join("runtime");
    runtime(target, &package, linker)?;
    let source = work.join("python");
    support::python(&package, &source)?;
    let built = work.join("built");
    run(Command::new(python).args(["-m", "build", "--wheel", "--outdir"]).arg(&built).arg(&source))?;
    let wheels: Vec<PathBuf> = io("cannot read the built wheel", fs::read_dir(&built))?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<_>>()
        .map_err(|e| e.to_string())?;
    let [wheel] = wheels.as_slice() else {
        return Err(format!("building lungo-py made {} wheels, not one", wheels.len()));
    };
    let retagged =
        run(Command::new(python).args(["-m", "wheel", "tags", "--platform-tag", tag, "--remove"]).arg(wheel))?;
    let name = String::from_utf8_lossy(&retagged.stdout).trim().to_owned();
    if !name.ends_with(&format!("-{tag}.whl")) {
        return Err(format!("retagging the wheel for {tag} made `{name}`"));
    }
    create_out(out_dir)?;
    let out = out_dir.join(&name);
    io(format!("cannot write {}", out.display()), fs::copy(built.join(&name), &out).map(|_| ()))?;
    io("cannot remove the scratch directory", fs::remove_dir_all(&work))?;
    println!("{}", out.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A release manifest publishing `platforms` of this version on `release`, with `name`d
    /// installers.
    fn release_manifest(platforms: &[(&str, String)]) -> String {
        let platforms: serde_json::Map<String, serde_json::Value> = platforms
            .iter()
            .map(|(p, name)| {
                (
                    p.to_string(),
                    serde_json::json!({ "label": p, "description": "", "minOS": "", "installers": [
                        { "type": "targz", "label": "tar.gz", "description": "", "name": name,
                          "path": format!("https://release.example.com/lungo-runtime/release/{VERSION}/{name}"),
                          "sha256": "ab".repeat(32), "size": 1 }
                    ]}),
                )
            })
            .collect();
        serde_json::json!({
            "schemaVersion": "5.0", "appId": "lungo-runtime", "name": "lungo runtime", "lastUpdated": "2026-01-01T00:00:00Z",
            "channels": {
                "release": { "latestVersion": VERSION, "versions": { VERSION: {
                    "publishedAt": "2026-01-01T00:00:00Z", "notesUrl": "https://release.example.com/notes.md", "platforms": platforms } } },
                "nightly": { "latestVersion": null, "versions": {} }
            }
        })
        .to_string()
    }

    fn complete() -> Vec<(&'static str, String)> {
        let mut all: Vec<(&str, String)> = RUNTIME_TARGETS.iter().map(|t| (*t, runtime_archive_name(t))).collect();
        all.push((XCFRAMEWORK_PLATFORM, xcframework_name()));
        all
    }

    /// TEST0261: the runtime manifest is what the release published
    #[test]
    fn test0261_the_runtime_manifest_is_what_the_release_published() {
        let m = runtime_manifest(&release_manifest(&complete()), "release").unwrap();
        assert_eq!(m["version"], VERSION);
        let artifacts = m["artifacts"].as_object().unwrap();
        assert_eq!(artifacts.len(), RUNTIME_TARGETS.len() + 1);
        assert_eq!(
            artifacts["wasm32-wasip1"]["url"],
            format!(
                "https://release.example.com/lungo-runtime/release/{VERSION}/{}",
                runtime_archive_name("wasm32-wasip1")
            )
        );
        assert_eq!(artifacts[XCFRAMEWORK_KEY]["sha256"], "ab".repeat(32));
    }

    /// TEST0262: an incomplete release names what is missing
    #[test]
    fn test0262_an_incomplete_release_names_what_is_missing() {
        let mut partial = complete();
        partial.retain(|(p, _)| *p != "x86_64-pc-windows-msvc" && *p != XCFRAMEWORK_PLATFORM);
        let e = runtime_manifest(&release_manifest(&partial), "release").unwrap_err();
        assert!(e.contains("x86_64-pc-windows-msvc") && e.contains(XCFRAMEWORK_PLATFORM), "{e}");
        // Another channel, or an installer of another name, is not this release's.
        assert!(runtime_manifest(&release_manifest(&complete()), "nightly").is_err());
        let mut renamed = complete();
        renamed[0].1 = "lungo-runtime-0.0.0-x.tar.gz".into();
        assert!(runtime_manifest(&release_manifest(&renamed), "release").unwrap_err().contains(RUNTIME_TARGETS[0]));
    }

    /// TEST0263: A local distribution's wheel is linked by this machine's toolchain and says so: on Linux
    /// it is tagged for this Linux, never `manylinux`, which is a claim only the zig-linked
    /// release runtime can make. `local` used to build its wheel the way a release does, so a
    /// Linux machine with no zig could not build a local distribution at all.
    #[test]
    fn test0263_a_local_wheel_is_tagged_for_this_machine_and_linked_by_it() {
        assert_eq!(local_wheel_tag("x86_64-unknown-linux-gnu").unwrap(), "linux_x86_64");
        assert_eq!(local_wheel_tag("aarch64-unknown-linux-gnu").unwrap(), "linux_aarch64");
        assert_eq!(release_wheel_tag("x86_64-unknown-linux-gnu").unwrap(), "manylinux_2_28_x86_64");
        for (target, tag) in WHEELS {
            let local = local_wheel_tag(target).unwrap();
            if target.contains("-linux-") {
                assert!(!local.contains("manylinux"), "{target}: a host-linked wheel is tagged {local}");
            } else {
                assert_eq!(local, *tag, "{target}");
            }
        }
        assert!(local_wheel_tag("wasm32-wasip1").is_err());

        // `local` builds through this, pinned by source: it runs cargo and Python, so it cannot
        // be called here, and a correct function nobody calls is the defect this replaces.
        let main = include_str!("main.rs");
        let local = &main[main.find("fn local(").expect("local")..];
        let local = &local[..local.find("\n}\n").expect("local ends")];
        assert!(local.contains("release::local_wheel("), "a local distribution's wheel is not the local one");
        assert!(!local.contains("release::wheel("), "a local distribution builds a release wheel");
        let source = include_str!("release.rs");
        let wheel = &source[source.find("pub fn local_wheel(").expect("local_wheel")..];
        let wheel = &wheel[..wheel.find("\n}\n").expect("local_wheel ends")];
        assert!(wheel.contains("Linker::Host"), "a local wheel is not linked by the host's toolchain");
    }

    /// TEST0264: every release target is linked by a toolchain that exists for it
    #[test]
    fn test0264_every_release_target_is_linked_by_a_toolchain_that_exists_for_it() {
        for t in RUNTIME_TARGETS {
            let apple_or_msvc = t.contains("-apple-") || t.ends_with("-windows-msvc");
            match linker_for(t) {
                Ok(_) => {}
                // Only the targets bound to their vendor's toolchain depend on the host.
                Err(e) => assert!(apple_or_msvc, "{t}: {e}"),
            }
        }
        assert!(linker_for("riscv64gc-unknown-none-elf").is_err());
        // The command and the wheels are built for release targets, whose runtimes they carry.
        let built = CLI_PLATFORMS.iter().map(|(_, t)| *t).chain(WHEELS.iter().map(|(t, _)| *t));
        for t in built {
            assert!(RUNTIME_TARGETS.contains(&t), "{t} is not a release target");
        }
    }
}
