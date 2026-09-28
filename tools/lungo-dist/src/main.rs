//! `lungo-dist`: builds lungo's distribution: a local one for development, and the artifacts of
//! a release, which the workspace's publishing runs one by one and records in the release
//! manifest it signs (see `release`).
//!
//! - `local`: a local distribution for this machine, laid out as a release is (`runtime/`,
//!   `wasm/`, and each language's support library), for `lungo generate --runtime-dir`, with
//!   lungo-py's wheel for this machine in `wheels/` for pip to find.
//! - `ts`: the npm package `lungo-ts`.
//! - `targets`: what a release is made of.
//! - `release-runtime`, `release-xcframework`, `release-cli`, `release-wheel`: one artifact.
//! - `manifest`: the runtime manifest the `lungo` command embeds, from the published release
//!   manifest.
//! - `release-go`, `release-swift`: the distribution repositories' trees, from the published
//!   runtime.

mod release;
mod support;

use clap::Parser;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

#[derive(Parser)]
#[command(name = "lungo-dist", about = "Builds lungo's distribution")]
enum Task {
    /// Build a local distribution for this machine.
    Local {
        #[arg(long)]
        out: PathBuf,
        /// Components to include (default: all but swift off macOS, and wheel without
        /// --python): runtime, wasm, go, python, wheel, swift, ts.
        #[arg(long = "component")]
        components: Vec<String>,
        /// The Python that builds lungo-py's wheel for this machine (the `wheel` component), into
        /// `wheels/`: a place for pip to find the unreleased lungo-py when it builds a package
        /// generated with this distribution.
        #[arg(long)]
        python: Option<PathBuf>,
    },
    /// Assemble the npm package `lungo-ts`.
    Ts {
        #[arg(long)]
        out: PathBuf,
    },
    /// Print what a release is made of, as JSON: the runtime targets, the `lungo` command's
    /// platforms, `lungo-py`'s wheels, the Go module's targets.
    Targets,
    /// Build the runtime archive of a release target into `--out-dir`.
    ReleaseRuntime {
        #[arg(long)]
        target: String,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Build the XCFramework of the Apple runtimes into `--out-dir` (on a Mac).
    ReleaseXcframework {
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Write the runtime manifest of this version from the published release manifest.
    Manifest {
        /// The release manifest of the runtime (`lungo-runtime/manifest`), as published.
        #[arg(long)]
        release_manifest: PathBuf,
        /// The channel the version was published on.
        #[arg(long)]
        channel: String,
        #[arg(long)]
        out: PathBuf,
    },
    /// Build the `lungo` command of a platform, embedding the runtime manifest, into `--out-dir`.
    ReleaseCli {
        #[arg(long)]
        platform: String,
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Build `lungo-py`'s wheel of a target into `--out-dir`.
    ReleaseWheel {
        #[arg(long)]
        target: String,
        /// The Python that builds it.
        #[arg(long)]
        python: PathBuf,
        #[arg(long)]
        out_dir: PathBuf,
    },
    /// Assemble the Go module `lungo-go` in `--out` from the runtimes the manifest lists.
    ReleaseGo {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
    /// Assemble the Swift package `lungo-swift` in `--out` for the XCFramework the manifest lists.
    ReleaseSwift {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        out: PathBuf,
    },
}

type Result<T> = std::result::Result<T, String>;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> ExitCode {
    let result = match Task::parse() {
        Task::Local { out, components, python } => local(&out, &components, python.as_deref()),
        Task::Ts { out } => support::typescript(&out),
        Task::Targets => release::targets(),
        Task::ReleaseRuntime { target, out_dir } => release::runtime_archive(&target, &out_dir),
        Task::ReleaseXcframework { out_dir } => release::xcframework(&out_dir),
        Task::Manifest { release_manifest, channel, out } => release::manifest(&release_manifest, &channel, &out),
        Task::ReleaseCli { platform, manifest, out_dir } => release::cli(&platform, &manifest, &out_dir),
        Task::ReleaseWheel { target, python, out_dir } => release::wheel(&target, &python, &out_dir),
        Task::ReleaseGo { manifest, out } => release::go(&manifest, &out),
        Task::ReleaseSwift { manifest, out } => release::swift(&manifest, &out),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("lungo-dist: {e}");
            ExitCode::FAILURE
        }
    }
}

/// The repository root. Paths are resolved with `dunce`: the tools lungo-dist runs (CMake,
/// MSBuild, dlltool) reject the verbatim `\\?\` paths `std` returns on Windows.
pub fn repo() -> PathBuf {
    dunce::canonicalize(Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")).expect("the repository exists")
}

/// Cargo's target directory: `CARGO_TARGET_DIR`, else the repository's `target/`.
pub fn target_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR").map(PathBuf::from).unwrap_or_else(|| repo().join("target"))
}

pub fn host() -> String {
    let out = Command::new("rustc").arg("-vV").output().expect("rustc runs");
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.lines().find_map(|l| l.strip_prefix("host: ")).expect("rustc reports its host").to_owned()
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
    } else if target.contains("-apple-ios") || target.starts_with("wasm32-") || target.ends_with("-linux-musl") {
        // No shared runtime: iOS and WebAssembly link statically, and musl targets build with
        // `crt-static`, under which Rust produces no shared library — a musl program is linked
        // whole, the runtime with it.
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

/// `cargo`, building the runtime for `target`: for Apple platforms at the supported deployment
/// targets; for Windows with `windows_raw_dylib`, so that the Windows API bindings import their
/// DLLs directly instead of through the import library the `windows-targets` crate carries in
/// Cargo's registry, which the programs linking the runtime do not have.
fn cargo(target: &str) -> Command {
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo())
        .env("CARGO_TARGET_DIR", runtime_target_dir())
        .env("MACOSX_DEPLOYMENT_TARGET", lungo_runtime::header::MACOS_DEPLOYMENT_TARGET)
        .env("IPHONEOS_DEPLOYMENT_TARGET", lungo_runtime::header::IOS_DEPLOYMENT_TARGET);
    let mut flags = remapped_paths();
    if target.contains("-windows-") {
        flags.extend(["--cfg".to_owned(), "windows_raw_dylib".to_owned()]);
    }
    // Encoded, so that no path is split on a space; it replaces every other RUSTFLAGS source.
    cmd.env("CARGO_ENCODED_RUSTFLAGS", flags.join("\u{1f}"));
    cmd
}

/// `--remap-path-prefix` for every directory of the building machine a runtime could name: the
/// toolchain's library sources, Cargo's home (the crates' sources) and the repository. A
/// runtime names source paths in its panic locations, and a published one must name none of
/// the machine it was built on — nor differ from a build of the same sources on another.
fn remapped_paths() -> Vec<String> {
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .expect("rustc prints its sysroot");
    let cargo_home = std::env::var_os("CARGO_HOME").map(PathBuf::from).unwrap_or_else(|| {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).expect("a home directory");
        PathBuf::from(home).join(".cargo")
    });
    [(PathBuf::from(sysroot), "/rustc"), (cargo_home, "/cargo"), (repo(), "/lungo")]
        .into_iter()
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .collect()
}

/// The native libraries the static runtime needs, as `rustc` reports them.
fn native_libraries(target: &str) -> Result<Vec<String>> {
    let out = run(cargo(target)
        .args(["rustc", "-p", "lungo-capi", "--release", "--target", target, "--crate-type", "staticlib", "--"])
        .args(["--print", "native-static-libs"]))?;
    let text = String::from_utf8_lossy(&out.stderr);
    let line = text
        .lines()
        .find_map(|l| l.split("native-static-libs:").nth(1))
        .ok_or("rustc did not report the native libraries of the runtime")?;
    // The C library every C toolchain links by default is left to it (listing it again makes
    // linkers warn; MSVC's `/defaultlib:msvcrt` is the default runtime of CMake's and every
    // other MSVC build); `-framework X` is one item for CMake.
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
        } else if !implicit.contains(&t) && !t.starts_with("/defaultlib:") && !items.iter().any(|i| i == t) {
            items.push(t.to_owned());
        }
    }
    if let Some(bundled) = items.iter().find(|i| i.contains("windows.0.")) {
        return Err(format!(
            "the runtime links `{bundled}`, the import library of the windows-targets crate, which exists only in Cargo's registry: it must be built with `--cfg windows_raw_dylib` (RUSTFLAGS, when set, overrides the target's)"
        ));
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

/// How a runtime is linked: by the host's toolchain, or by `cargo zigbuild` (zig's linker and
/// C library headers), which links Linux and Windows GNU targets from any host — against
/// glibc [`release::GLIBC`] for the Linux GNU targets, so the shared runtime loads on every
/// manylinux system its Python wheel claims.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Linker {
    Host,
    Zig,
}

/// Builds the runtime package of `target` into `out` (replaced).
pub fn runtime(target: &str, out: &Path, linker: Linker) -> Result<()> {
    let repo = repo();
    match linker {
        Linker::Host => run(cargo(target).args(["build", "-p", "lungo-capi", "--release", "--target", target]))?,
        Linker::Zig => {
            release::check_zig(target)?;
            let zig_target =
                if target.ends_with("-linux-gnu") { format!("{target}.{}", release::GLIBC) } else { target.to_owned() };
            run(cargo(target).args(["zigbuild", "-p", "lungo-capi", "--release", "--target", &zig_target]))?
        }
    };
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

/// A local distribution for this machine. Each component it builds replaces the one there.
fn local(out: &Path, components: &[String], python: Option<&Path>) -> Result<()> {
    const ALL: &[&str] = &["runtime", "wasm", "go", "python", "wheel", "swift", "ts"];
    for c in components {
        if !ALL.contains(&c.as_str()) {
            return Err(format!("unknown component {c} (components: {})", ALL.join(", ")));
        }
    }
    let host = host();
    let apple = host.contains("-apple-darwin");
    // By default every component this machine can build: the Swift package only on a Mac, the
    // wheel only with a Python to build it.
    let wanted = |c: &str| {
        if components.is_empty() {
            (c != "swift" || apple) && (c != "wheel" || python.is_some())
        } else {
            components.iter().any(|x| x == c)
        }
    };
    io(format!("cannot create {}", out.display()), fs::create_dir_all(out))?;
    let out = io("cannot resolve the output", dunce::canonicalize(out))?;
    if wanted("runtime") || wanted("go") || wanted("python") || wanted("swift") {
        runtime(&host, &out.join("runtime"), Linker::Host)?;
    }
    // The TypeScript binding links the WebAssembly runtime into its packages.
    if wanted("wasm") || wanted("ts") {
        runtime("wasm32-wasip1", &out.join("wasm"), Linker::Host)?;
    }
    if wanted("ts") {
        support::typescript(&out.join("ts"))?;
    }
    if wanted("go") {
        // cgo links with MinGW on Windows: the Go module carries the GNU runtime.
        let go_target = host.replace("-pc-windows-msvc", "-pc-windows-gnu");
        let go_runtime = if go_target == host {
            out.join("runtime")
        } else {
            runtime(&go_target, &out.join("go-runtime"), Linker::Host)?;
            out.join("go-runtime")
        };
        support::go(&[(go_target, go_runtime)], &out.join("go"))?;
    }
    if wanted("python") {
        support::python(&out.join("runtime"), &out.join("python"))?;
    }
    if wanted("wheel") {
        let python = python.ok_or("the wheel component is built with --python")?;
        let wheels = out.join("wheels");
        if wheels.exists() {
            io(format!("cannot replace {}", wheels.display()), fs::remove_dir_all(&wheels))?;
        }
        release::wheel(&host, python, &wheels)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A musl runtime is static only: musl targets build with `crt-static`, under which Rust
    /// makes no shared library, so a package that expected one could not be assembled.
    #[test]
    fn a_musl_runtime_is_static_only() {
        for target in ["x86_64-unknown-linux-musl", "aarch64-unknown-linux-musl"] {
            let libs = libraries(target);
            assert_eq!(libs.built_static, "liblungo.a");
            assert!(libs.built_shared.is_none() && libs.shared_dest.is_none(), "{target}");
        }
        // The GNU targets keep theirs: a Python wheel loads it.
        let gnu = libraries("x86_64-unknown-linux-gnu");
        assert_eq!(gnu.shared_dest, Some("lib/liblungo.so"));
    }
}
