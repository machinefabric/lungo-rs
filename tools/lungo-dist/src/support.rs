//! The language support libraries, assembled with the runtime for distribution: the Go module
//! (`lungo-go`), the Python package (`lungo-py`), the Swift package (`lungo-swift`), the npm
//! package (`lungo-ts`).

use crate::{Result, VERSION, io, repo};
use std::fs;
use std::path::{Path, PathBuf};

/// Copies the files of `from` into `to`, recursively, except those `skip` rejects (by path
/// relative to `from`).
pub fn copy_tree(from: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<()> {
    fn walk(root: &Path, dir: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<()> {
        for e in io(format!("cannot read {}", dir.display()), fs::read_dir(dir))? {
            let path = io("cannot read a directory entry", e)?.path();
            let rel = path.strip_prefix(root).expect("walked paths are under the root");
            if skip(rel) {
                continue;
            }
            let dest = to.join(rel);
            if path.is_dir() {
                io(format!("cannot create {}", dest.display()), fs::create_dir_all(&dest))?;
                walk(root, &path, to, skip)?;
            } else {
                io(format!("cannot copy {}", path.display()), fs::copy(&path, &dest).map(|_| ()))?;
            }
        }
        Ok(())
    }
    io(format!("cannot create {}", to.display()), fs::create_dir_all(to))?;
    walk(from, from, to, skip)
}

/// A support library's sources: its own repository, beside this one in the lungo repository
/// (whose submodules they all are).
fn library(name: &str) -> Result<PathBuf> {
    let root = repo();
    let dir = root
        .parent()
        .ok_or_else(|| format!("{} has no parent to find {name} beside", root.display()))?
        .join(name);
    if !dir.is_dir() {
        return Err(format!(
            "{name} is not checked out at {}: the support libraries are submodules of the lungo \
             repository, beside lungo-rs; clone it with --recurse-submodules",
            dir.display()
        ));
    }
    Ok(dir)
}

/// Copies a support library's files into `to`, except those `skip` rejects (by path relative to
/// the library).
///
/// Its files are what its repository holds — tracked, or new and not ignored — as git lists
/// them: never what a checkout collects beside them (the repository itself, build output, the
/// tools' state, caches), whatever those are called.
fn copy_library(dir: &Path, to: &Path, skip: &dyn Fn(&Path) -> bool) -> Result<()> {
    let listed = io(
        format!("cannot run git in {}", dir.display()),
        std::process::Command::new("git")
            .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
            .current_dir(dir)
            .output(),
    )?;
    if !listed.status.success() {
        return Err(format!(
            "git cannot list the files of {}: {}",
            dir.display(),
            String::from_utf8_lossy(&listed.stderr).trim()
        ));
    }
    io(format!("cannot create {}", to.display()), fs::create_dir_all(to))?;
    for rel in listed.stdout.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let rel = Path::new(std::str::from_utf8(rel).map_err(|_| format!("{} lists a path that is not UTF-8", dir.display()))?);
        let path = dir.join(rel);
        // Tracked, and deleted in the checkout: not a file of it now. And `version.txt` at its
        // root is the workspace's record of the repository's own version, not the lungo
        // version a release of it carries.
        if skip(rel) || !path.is_file() || rel == Path::new("version.txt") {
            continue;
        }
        let dest = to.join(rel);
        if let Some(parent) = dest.parent() {
            io(format!("cannot create {}", parent.display()), fs::create_dir_all(parent))?;
        }
        io(format!("cannot copy {}", path.display()), fs::copy(&path, &dest).map(|_| ()))?;
    }
    Ok(())
}

/// The native libraries of a runtime package, from its `lungo.pc`.
fn native_libraries(runtime: &Path) -> Result<String> {
    let pc = runtime.join("lib/pkgconfig/lungo.pc");
    let text = io(format!("cannot read {}", pc.display()), fs::read_to_string(&pc))?;
    text.lines()
        .find_map(|l| l.strip_prefix("Libs.private:"))
        .map(|l| l.trim().to_owned())
        .ok_or_else(|| format!("{} has no Libs.private", pc.display()))
}

/// The Go platform (`GOOS`, `GOARCH`) of a runtime target, for the targets Go links with cgo.
pub fn go_platform(target: &str) -> Option<(&'static str, &'static str)> {
    Some(match target {
        "aarch64-apple-darwin" => ("darwin", "arm64"),
        "x86_64-apple-darwin" => ("darwin", "amd64"),
        "x86_64-unknown-linux-gnu" => ("linux", "amd64"),
        "aarch64-unknown-linux-gnu" => ("linux", "arm64"),
        "x86_64-pc-windows-gnu" => ("windows", "amd64"),
        _ => return None,
    })
}

/// The Go module `lungo-go` with the runtimes `runtimes` (target, runtime package), in `out`
/// (replaced).
pub fn go(runtimes: &[(String, PathBuf)], out: &Path) -> Result<()> {
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    let repo = repo();
    copy_library(&library("lungo-go")?, out, &|_| false)?;
    io("cannot copy LICENSE", fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map(|_| ()))?;
    io("cannot create include/", fs::create_dir_all(out.join("include")))?;
    io("cannot write lungo.h", fs::write(out.join("include/lungo.h"), lungo_runtime::header::HEADER))?;
    io("cannot create testdata/", fs::create_dir_all(out.join("testdata")))?;
    io(
        "cannot copy the wire vectors",
        fs::copy(repo.join("compiler-tests/wire/vectors.json"), out.join("testdata/vectors.json")).map(|_| ()),
    )?;
    let lock = VERSION.replace('.', "_");
    io(
        "cannot write version.go",
        fs::write(
            out.join("version.go"),
            format!(
                "// Code generated by lungo-dist; DO NOT EDIT.\n\npackage lungo\n\n// Version is the lungo release of this module.\nconst Version = \"{VERSION}\"\n\n// EnforceVersion{lock} is referenced by the packages lungo {VERSION} generates, which\n// build only with this module's version.\nconst EnforceVersion{lock} = 0\n"
            ),
        ),
    )?;
    if runtimes.is_empty() {
        return Err("the Go module needs at least one runtime".into());
    }
    for (target, runtime) in runtimes {
        let (goos, goarch) =
            go_platform(target).ok_or_else(|| format!("Go does not link the runtime of {target} with cgo"))?;
        let platform = format!("{goos}_{goarch}");
        let lib = out.join("lib").join(&platform);
        io(format!("cannot create {}", lib.display()), fs::create_dir_all(&lib))?;
        io(
            format!("cannot copy the runtime of {target}"),
            fs::copy(runtime.join("lib/liblungo.a"), lib.join("liblungo.a")).map(|_| ()),
        )?;
        let natives = native_libraries(runtime)?;
        io(
            "cannot write a link file",
            fs::write(
                out.join(format!("link_{platform}.go")),
                format!(
                    "// Code generated by lungo-dist; DO NOT EDIT.\n\n//go:build {goos} && {goarch}\n\npackage lungo\n\n// #cgo LDFLAGS: ${{SRCDIR}}/lib/{platform}/liblungo.a {natives}\nimport \"C\"\n"
                ),
            ),
        )?;
    }
    Ok(())
}

/// The Python package `lungo-py` (a source tree) with the runtime package `runtime`, in `out`
/// (replaced). The wheel carries the shared runtime library, its header and its CMake package.
pub fn python(runtime: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    let repo = repo();
    copy_library(&library("lungo-py")?, out, &|_| false)?;
    io("cannot copy LICENSE", fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map(|_| ()))?;
    io(
        "cannot write _version.py",
        fs::write(out.join("src/lungo_py/_version.py"), format!("__version__ = \"{VERSION}\"\n")),
    )?;
    io(
        "cannot copy the wire vectors",
        fs::copy(repo.join("compiler-tests/wire/vectors.json"), out.join("tests/vectors.json")).map(|_| ()),
    )?;
    // The runtime without its static library: Python loads the shared one.
    copy_tree(runtime, &out.join("src/lungo_py/runtime"), &|rel| {
        let name = rel.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        name == "liblungo.a" || name == "lungo.lib"
    })?;
    Ok(())
}

/// The module map of the runtime's headers in the XCFramework: Swift imports `LungoRuntime`.
const MODULE_MAP: &str = "module LungoRuntime {\n    header \"lungo.h\"\n    export *\n}\n";

/// `LungoRuntime.xcframework` in `out` from the static runtime libraries `libraries` (one per
/// platform variant: macOS, iOS, iOS simulator; each may be universal).
pub fn xcframework(libraries: &[PathBuf], out: &Path) -> Result<()> {
    let work = out.parent().expect("the output has a parent").join(".xcframework-headers");
    if work.exists() {
        io("cannot clear the headers", fs::remove_dir_all(&work))?;
    }
    io("cannot create the headers", fs::create_dir_all(&work))?;
    io("cannot write lungo.h", fs::write(work.join("lungo.h"), lungo_runtime::header::HEADER))?;
    io("cannot write the module map", fs::write(work.join("module.modulemap"), MODULE_MAP))?;
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    let mut cmd = std::process::Command::new("xcodebuild");
    cmd.arg("-create-xcframework");
    for lib in libraries {
        cmd.arg("-library").arg(lib).arg("-headers").arg(&work);
    }
    cmd.arg("-output").arg(out);
    crate::run(&mut cmd)?;
    io("cannot remove the headers", fs::remove_dir_all(&work))?;
    Ok(())
}

/// SwiftPM linker settings of the native libraries of a runtime package.
fn swift_linker_settings(runtime: &Path) -> Result<String> {
    let natives = native_libraries(runtime)?;
    let mut settings = Vec::new();
    let mut tokens = natives.split_whitespace();
    while let Some(t) = tokens.next() {
        if t == "-framework" {
            let name = tokens.next().ok_or("`-framework` without a name")?;
            settings.push(format!(".linkedFramework(\"{name}\")"));
        } else if let Some(lib) = t.strip_prefix("-l") {
            settings.push(format!(".linkedLibrary(\"{lib}\")"));
        } else {
            return Err(format!("the native library `{t}` has no SwiftPM linker setting"));
        }
    }
    Ok(settings.join(", "))
}

/// The Swift package `lungo-swift` in `out` (a new directory, or one holding only its local
/// XCFramework), whose runtime target is `runtime_target`
/// (a `.binaryTarget` with a path or a URL and checksum), linking the native libraries of
/// `runtime`.
pub fn swift(runtime_target: &str, runtime: &Path, out: &Path) -> Result<()> {
    let repo = repo();
    let swift = library("lungo-swift")?;
    copy_library(&swift, out, &|rel| rel == Path::new("Package.swift.in"))?;
    io("cannot copy LICENSE", fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map(|_| ()))?;
    io(
        "cannot copy the wire vectors",
        fs::copy(repo.join("compiler-tests/wire/vectors.json"), out.join("Tests/LungoKitTests/vectors.json"))
            .map(|_| ()),
    )?;
    let template =
        io("cannot read Package.swift.in", fs::read_to_string(swift.join("Package.swift.in")))?;
    let linker = swift_linker_settings(runtime)?;
    let manifest = crate::fill(
        &template,
        &[
            ("VERSION", VERSION),
            ("RUNTIME_TARGET", runtime_target),
            ("LINKER_SETTINGS", &linker),
            ("MACOS", lungo_runtime::header::MACOS_DEPLOYMENT_TARGET),
            ("IOS", lungo_runtime::header::IOS_DEPLOYMENT_TARGET),
        ],
    )?;
    io("cannot write Package.swift", fs::write(out.join("Package.swift"), manifest))?;
    Ok(())
}

/// The Swift package of a local distribution: an XCFramework of this Mac's runtime.
pub fn swift_local(runtime: &Path, out: &Path) -> Result<()> {
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    io(format!("cannot create {}", out.display()), fs::create_dir_all(out))?;
    xcframework(&[runtime.join("lib/liblungo.a")], &out.join("LungoRuntime.xcframework"))?;
    swift(".binaryTarget(name: \"LungoRuntime\", path: \"LungoRuntime.xcframework\")", runtime, out)
}

/// The npm package `lungo-ts` in `out` (replaced).
pub fn typescript(out: &Path) -> Result<()> {
    if out.exists() {
        io(format!("cannot replace {}", out.display()), fs::remove_dir_all(out))?;
    }
    let repo = repo();
    let typescript = library("lungo-ts")?;
    copy_library(&typescript, out, &|rel| rel == Path::new("package.json.in"))?;
    let template = io("cannot read package.json.in", fs::read_to_string(typescript.join("package.json.in")))?;
    io(
        "cannot write package.json",
        fs::write(out.join("package.json"), crate::fill(&template, &[("VERSION", VERSION)])?),
    )?;
    io("cannot copy LICENSE", fs::copy(repo.join("LICENSE"), out.join("LICENSE")).map(|_| ()))?;
    io(
        "cannot copy the wire vectors",
        fs::copy(repo.join("compiler-tests/wire/vectors.json"), out.join("test/vectors.json")).map(|_| ()),
    )?;
    Ok(())
}
