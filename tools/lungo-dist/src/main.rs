//! `lungo-dist`: builds lungo's distribution, for the release workflow and for local use.
//!
//! - `runtime`: the runtime package of a target (`include/lungo.h`, the static and shared
//!   libraries, `lib/cmake/lungo/`, `lib/pkgconfig/lungo.pc`, `LICENSE`, `VERSION`).
//! - `local`: a local distribution for this machine, laid out as a release is (`runtime/`,
//!   `wasm/`, and each language's support library), for `lungo generate --runtime-dir`.
//! - `archive`: a reproducible `.tar.gz` of a directory.
//! - `manifest`: the release's `runtime-manifest.json` and `SHA256SUMS`.

mod support;

use clap::Parser;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(name = "lungo-dist", about = "Builds lungo's distribution")]
enum Cli {
    /// Build the runtime package of a target.
    Runtime {
        /// The target triple (default: this machine's).
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build a local distribution for this machine.
    Local {
        #[arg(long)]
        out: PathBuf,
        /// Components to include (default: all but swift off macOS): runtime, go, python, swift.
        #[arg(long = "component")]
        components: Vec<String>,
    },
    /// Write a reproducible `.tar.gz` of a directory, whose entries are under its name.
    Archive {
        dir: PathBuf,
        out: PathBuf,
    },
    /// Assemble the Go module `lungo-go` with runtime packages (`--runtime TARGET=DIR`).
    Go {
        #[arg(long = "runtime", value_parser = target_dir_pair)]
        runtimes: Vec<(String, PathBuf)>,
        #[arg(long)]
        out: PathBuf,
    },
    /// Assemble the Python package `lungo-py` (a source tree) with a runtime package.
    Python {
        #[arg(long)]
        runtime: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Write `runtime-manifest.json` and `SHA256SUMS` for release artifacts.
    Manifest {
        /// The directory holding the artifacts (`lungo-runtime-<version>-<target>.tar.gz`,
        /// `LungoRuntime.xcframework.zip`).
        #[arg(long)]
        artifacts: PathBuf,
        /// The URL the artifacts are downloaded from.
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        out: PathBuf,
    },
}

type Result<T> = std::result::Result<T, String>;

/// `TARGET=DIR`.
fn target_dir_pair(s: &str) -> Result<(String, PathBuf)> {
    let (t, d) = s.split_once('=').ok_or_else(|| format!("`{s}` is not TARGET=DIR"))?;
    Ok((t.to_owned(), PathBuf::from(d)))
}

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let result = match Cli::parse() {
        Cli::Runtime { target, out } => runtime(&target.unwrap_or_else(host), &out),
        Cli::Local { out, components } => local(&out, &components),
        Cli::Archive { dir, out } => archive(&dir, &out),
        Cli::Go { runtimes, out } => support::go(&runtimes, &out),
        Cli::Python { runtime, out } => support::python(&runtime, &out),
        Cli::Manifest { artifacts, base_url, out } => manifest(&artifacts, &base_url, &out),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lungo-dist: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root.
pub fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("the repository exists")
}

/// Cargo's target directory: `CARGO_TARGET_DIR`, else the repository's `target/`.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("target"))
}

fn host() -> String {
    let out = Command::new("rustc").arg("-vV").output().expect("rustc runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines()
        .find_map(|l| l.strip_prefix("host: "))
        .expect("rustc reports its host")
        .to_owned()
}

pub fn io<T>(what: impl std::fmt::Display, r: std::io::Result<T>) -> Result<T> {
    r.map_err(|e| format!("{what}: {e}"))
}

pub fn run(cmd: &mut Command) -> Result<std::process::Output> {
    let shown = format!("{cmd:?}");
    let out = io(format!("cannot run {shown}"), cmd.output())?;
    if !out.status.success() {
        return Err(format!(
            "{shown} failed ({}):\n{}{}",
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ));
    }
    Ok(out)
}

/// The file names of the runtime libraries of `target`: static, shared (relative to the
/// package), and the import library of the shared one.
struct Libraries {
    built_static: &'static str,
    built_shared: Option<&'static str>,
    built_import: Option<&'static str>,
    shared_dest: Option<&'static str>,
}

fn libraries(target: &str) -> Libraries {
    if target.ends_with("-windows-msvc") {
        Libraries {
            built_static: "lungo.lib",
            built_shared: Some("lungo.dll"),
            built_import: Some("lungo.dll.lib"),
            shared_dest: Some("bin/lungo.dll"),
        }
    } else if target.ends_with("-windows-gnu") {
        Libraries {
            built_static: "liblungo.a",
            built_shared: Some("lungo.dll"),
            built_import: Some("liblungo.dll.a"),
            shared_dest: Some("bin/lungo.dll"),
        }
    } else if target.contains("-apple-ios") || target.starts_with("wasm32-") {
        Libraries { built_static: "liblungo.a", built_shared: None, built_import: None, shared_dest: None }
    } else if target.contains("-apple-") {
        Libraries {
            built_static: "liblungo.a",
            built_shared: Some("liblungo.dylib"),
            built_import: None,
            shared_dest: Some("lib/liblungo.dylib"),
        }
    } else {
        Libraries {
            built_static: "liblungo.a",
            built_shared: Some("liblungo.so"),
            built_import: None,
            shared_dest: Some("lib/liblungo.so"),
        }
    }
}

/// Where the runtime is built: a target directory of its own, since Cargo does not rebuild
/// dependencies when the deployment target changes.
fn runtime_target_dir() -> PathBuf {
    target_dir().join("lungo-dist")
}

/// `cargo`, building for Apple platforms at the supported deployment targets.
fn cargo() -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo())
        .env("CARGO_TARGET_DIR", runtime_target_dir())
        .env("MACOSX_DEPLOYMENT_TARGET", lungo_runtime::header::MACOS_DEPLOYMENT_TARGET)
        .env("IPHONEOS_DEPLOYMENT_TARGET", lungo_runtime::header::IOS_DEPLOYMENT_TARGET);
    cmd
}

/// The native libraries the static runtime needs, as `rustc` reports them.
fn native_libraries(target: &str) -> Result<Vec<String>> {
    let out = run(cargo()
        .args(["rustc", "-p", "lungo-capi", "--release", "--target", target, "--crate-type", "staticlib", "--"])
        .args(["--print", "native-static-libs"]))?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text
        .lines()
        .find_map(|l| l.split("native-static-libs:").nth(1))
        .ok_or("rustc did not report the native libraries of the runtime")?;
    // The C library every C toolchain links by default is left to it (listing it again makes
    // linkers warn); `-framework X` is one item for CMake.
    let implicit: &[&str] = if target.contains("-apple-") {
        &["-lSystem", "-lc", "-lm"]
    } else if target.contains("-linux-") {
        &["-lc"]
    } else {
        &[]
    };
    let mut items: Vec<String> = Vec::new();
    let mut tokens = line.split_whitespace();
    while let Some(t) = tokens.next() {
        if t == "-framework" {
            items.push(format!("-framework {}", tokens.next().ok_or("`-framework` without a name")?));
        } else if !implicit.contains(&t) && !items.iter().any(|i| i == t) {
            items.push(t.to_owned());
        }
    }
    Ok(items)
}

pub fn fill(template: &str, values: &[(&str, &str)]) -> Result<String> {
    let mut s = template.to_owned();
    for (k, v) in values {
        s = s.replace(&format!("@{k}@"), v);
    }
    if let Some(p) = placeholder(&s) {
        return Err(format!("the template placeholder @{p}@ has no value"));
    }
    Ok(s)
}

/// The first `@NAME@` placeholder (uppercase letters and `_`) in `s`.
fn placeholder(s: &str) -> Option<&str> {
    let mut rest = s;
    while let Some(i) = rest.find('@') {
        let after = &rest[i + 1..];
        if let Some(j) = after.find('@') {
            let name = &after[..j];
            if !name.is_empty() && name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
                return Some(name);
            }
        }
        rest = after;
    }
    None
}

/// Builds the runtime package of `target` into `out` (replaced).
fn runtime(target: &str, out: &Path) -> Result<()> {
    let repo = repo();
    run(cargo().args(["build", "-p", "lungo-capi", "--release", "--target", target]))?;
    let natives = native_libraries(target)?;
    let built = runtime_target_dir().join(target).join("release");
    let libs = libraries(target);
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    for d in ["include", "lib/cmake/lungo", "lib/pkgconfig", "bin"] {
        io("cannot create the package", fs::create_dir_all(out.join(d)))?;
    }
    io("cannot write lungo.h", fs::write(out.join("include/lungo.h"), lungo_runtime::header::HEADER))?;
    let copy = |from: &str, to: &str| -> Result<()> {
        io(format!("cannot copy {from}"), fs::copy(built.join(from), out.join(to)).map(|_| ()))
    };
    copy(libs.built_static, &format!("lib/{}", libs.built_static))?;
    if let (Some(shared), Some(dest)) = (libs.built_shared, libs.shared_dest) {
        copy(shared, dest)?;
    }
    if let Some(import) = libs.built_import {
        copy(import, &format!("lib/{import}"))?;
    }
    if fs::read_dir(out.join("bin")).map(|mut d| d.next().is_none()).unwrap_or(false) {
        io("cannot remove bin/", fs::remove_dir(out.join("bin")))?;
    }
    let pointer_bytes = if target.starts_with("wasm32-") { "4" } else { "8" };
    let natives_cmake = natives.join(";");
    let natives_pc = natives.join(" ");
    let values = [
        ("VERSION", VERSION),
        ("TARGET", target),
        ("STATIC_LIBRARY", libs.built_static),
        ("SHARED_LIBRARY", libs.shared_dest.unwrap_or("")),
        ("IMPORT_LIBRARY", libs.built_import.unwrap_or("")),
        ("NATIVE_LIBRARIES", &natives_cmake),
        ("NATIVE_LIBRARIES_PC", &natives_pc),
        ("POINTER_BYTES", pointer_bytes),
    ];
    let templates = repo.join("runtimes/c");
    for (template, dest) in [
        ("lungoConfig.cmake.in", "lib/cmake/lungo/lungoConfig.cmake"),
        ("lungoConfigVersion.cmake.in", "lib/cmake/lungo/lungoConfigVersion.cmake"),
        ("lungo.pc.in", "lib/pkgconfig/lungo.pc"),
    ] {
        let text = io(format!("cannot read {template}"), fs::read_to_string(templates.join(template)))?;
        io(format!("cannot write {dest}"), fs::write(out.join(dest), fill(&text, &values)?))?;
    }
    io("cannot copy LICENSE", fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map(|_| ()))?;
    io("cannot write VERSION", fs::write(out.join("VERSION"), format!("{VERSION}\n")))?;
    println!("{}", out.display());
    Ok(())
}

/// A local distribution for this machine.
fn local(out: &Path, components: &[String]) -> Result<()> {
    const ALL: &[&str] = &["runtime", "go", "python", "swift"];
    for c in components {
        if !ALL.contains(&c.as_str()) {
            return Err(format!("unknown component {c} (components: {})", ALL.join(", ")));
        }
    }
    let host = host();
    let apple = host.contains("-apple-darwin");
    // By default every component this machine can build: the Swift package only on a Mac.
    let wanted =
        |c: &str| if components.is_empty() { c != "swift" || apple } else { components.iter().any(|x| x == c) };
    io(format!("cannot create {}", out.display()), fs::create_dir_all(out))?;
    let out = io("cannot resolve the output", out.canonicalize())?;
    if wanted("runtime") || wanted("go") || wanted("python") || wanted("swift") {
        runtime(&host, &out.join("runtime"))?;
    }
    if wanted("go") {
        support::go(&[(host.clone(), out.join("runtime"))], &out.join("go"))?;
    }
    if wanted("python") {
        support::python(&out.join("runtime"), &out.join("python"))?;
    }
    if wanted("swift") {
        if !apple {
            return Err("the Swift package needs a Mac (its runtime is an XCFramework)".into());
        }
        support::swift_local(&out.join("runtime"), &out.join("lungo-swift"))?;
    }
    io("cannot write VERSION", fs::write(out.join("VERSION"), format!("{VERSION}\n")))?;
    println!("{}", out.display());
    Ok(())
}

/// A reproducible `.tar.gz` of `dir`: entries sorted, under the directory's name, with fixed
/// times, owners and modes (executables keep their execute bits).
pub fn archive(dir: &Path, out: &Path) -> Result<()> {
    let name = dir.file_name().ok_or("the directory has no name")?.to_string_lossy().into_owned();
    let file = io(format!("cannot create {}", out.display()), fs::File::create(out))?;
    let gz = flate2::GzBuilder::new().mtime(0).write(file, flate2::Compression::best());
    let mut b = tar::Builder::new(gz);
    let mut entries = Vec::new();
    collect(dir, Path::new(&name), &mut entries)?;
    entries.sort();
    for (rel, path) in entries {
        let meta = io(format!("cannot read {}", path.display()), fs::metadata(&path))?;
        let mut h = tar::Header::new_gnu();
        h.set_mtime(0);
        h.set_uid(0);
        h.set_gid(0);
        if meta.is_dir() {
            h.set_entry_type(tar::EntryType::Directory);
            h.set_mode(0o755);
            h.set_size(0);
            h.set_cksum();
            io("cannot archive", b.append_data(&mut h, &rel, std::io::empty()))?;
        } else {
            let bytes = io(format!("cannot read {}", path.display()), fs::read(&path))?;
            h.set_mode(if executable(&meta) { 0o755 } else { 0o644 });
            h.set_size(bytes.len() as u64);
            h.set_cksum();
            io("cannot archive", b.append_data(&mut h, &rel, bytes.as_slice()))?;
        }
    }
    let gz = io("cannot finish the archive", b.into_inner())?;
    io("cannot finish the archive", gz.finish().map(|_| ()))?;
    Ok(())
}

#[cfg(unix)]
fn executable(meta: &fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn executable(_: &fs::Metadata) -> bool {
    false
}

fn collect(dir: &Path, rel: &Path, out: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    out.push((rel.to_path_buf(), dir.to_path_buf()));
    for e in io(format!("cannot read {}", dir.display()), fs::read_dir(dir))? {
        let e = io("cannot read a directory entry", e)?;
        let path = e.path();
        let name = rel.join(e.file_name());
        if path.is_dir() {
            collect(&path, &name, out)?;
        } else {
            out.push((name, path));
        }
    }
    Ok(())
}

pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = io(format!("cannot read {}", path.display()), fs::read(path))?;
    Ok(Sha256::digest(&bytes).iter().map(|b| format!("{b:02x}")).collect())
}

/// `runtime-manifest.json` and `SHA256SUMS` of the release artifacts in `dir`.
fn manifest(dir: &Path, base_url: &str, out: &Path) -> Result<()> {
    let prefix = format!("lungo-runtime-{VERSION}-");
    let mut artifacts = BTreeMap::new();
    let mut sums = String::new();
    let mut names: Vec<String> = io(format!("cannot read {}", dir.display()), fs::read_dir(dir))?
        .map(|e| e.map(|e| e.file_name().to_string_lossy().into_owned()))
        .collect::<std::io::Result<_>>()
        .map_err(|e| e.to_string())?;
    names.sort();
    for name in names {
        let key = if let Some(target) = name.strip_prefix(&prefix).and_then(|n| n.strip_suffix(".tar.gz")) {
            target.to_owned()
        } else if name == "LungoRuntime.xcframework.zip" {
            "xcframework".to_owned()
        } else {
            continue;
        };
        let sha = sha256_file(&dir.join(&name))?;
        sums.push_str(&format!("{sha}  {name}\n"));
        artifacts.insert(
            key,
            serde_json::json!({ "url": format!("{}/{name}", base_url.trim_end_matches('/')), "sha256": sha }),
        );
    }
    if artifacts.is_empty() {
        return Err(format!("{} holds no release artifacts", dir.display()));
    }
    let manifest = serde_json::json!({
        "version": VERSION,
        "abi_version": lungo_runtime::ABI_VERSION,
        "artifacts": artifacts,
    });
    io("cannot create the output", fs::create_dir_all(out))?;
    let mut f = io("cannot write the manifest", fs::File::create(out.join("runtime-manifest.json")))?;
    io("cannot write the manifest", writeln!(f, "{}", serde_json::to_string_pretty(&manifest).expect("JSON")))?;
    io("cannot write SHA256SUMS", fs::write(out.join("SHA256SUMS"), sums))?;
    Ok(())
}
