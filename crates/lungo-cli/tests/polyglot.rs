//! End-to-end tests of every language binding: the polyglot fixture
//! (`compiler-tests/polyglot`) is generated for each language with a local distribution built
//! from this repository, the generated package is built with the language's own toolchain, and
//! the language's test program makes the same assertions on it.
//!
//! Each test requires its language's toolchain (CMake and a C compiler, Go, Python, Swift,
//! Node.js) and fails when it is missing.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// Where the tests work: the fixture's distribution, generated packages and builds.
fn root() -> PathBuf {
    repo().join("target").join("polyglot-e2e")
}

#[track_caller]
fn run(cmd: &mut Command) -> String {
    let shown = format!("{cmd:?}");
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {shown}: {e} (is the toolchain installed?)"));
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(
        out.status.success(),
        "{shown} failed ({}):\n{stdout}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    stdout
}

/// The local distribution with `components`, built once per component set with its own Cargo
/// target directory (the outer `cargo test` holds the repository's).
fn distribution(components: &[&str]) -> PathBuf {
    static BUILT: OnceLock<std::sync::Mutex<Vec<String>>> = OnceLock::new();
    let built = BUILT.get_or_init(Default::default);
    let mut built = built.lock().unwrap_or_else(|p| p.into_inner());
    let dist = root().join("dist");
    let missing: Vec<&str> = components.iter().copied().filter(|c| !built.iter().any(|b| b == c)).collect();
    if !missing.is_empty() {
        let mut cmd = Command::new(env!("CARGO"));
        cmd.current_dir(repo())
            .env("CARGO_TARGET_DIR", root().join("cargo"))
            .args(["run", "--quiet", "-p", "lungo-dist", "--", "local", "--out"])
            .arg(&dist);
        for c in &missing {
            cmd.args(["--component", c]);
        }
        run(&mut cmd);
        built.extend(missing.iter().map(|c| c.to_string()));
    }
    dist
}

/// Generates the fixture for `language` with the distribution components `components`.
fn generate(language: &str, components: &[&str]) -> PathBuf {
    let dist = distribution(components);
    let out = root().join(language);
    run(Command::new(env!("CARGO_BIN_EXE_lungo"))
        .arg("--config")
        .arg(repo().join("compiler-tests/polyglot/lungo.toml"))
        .arg("generate")
        .arg(format!("--{language}_out={}", out.display()))
        .arg("--runtime-dir")
        .arg(&dist));
    out
}

fn fixture(language: &str) -> PathBuf {
    repo().join("compiler-tests/polyglot").join(language)
}

#[test]
fn c_binding() {
    let package = generate("c", &["runtime"]);
    let build = root().join("c-build");
    run(Command::new("cmake")
        .arg("-S")
        .arg(fixture("c"))
        .arg("-B")
        .arg(&build)
        .arg(format!("-DPOLYGLOT_PACKAGE={}", package.display()))
        .arg("-DCMAKE_BUILD_TYPE=Debug"));
    run(Command::new("cmake").arg("--build").arg(&build).args(["--config", "Debug", "--parallel"]));
    let exe = ["polyglot_test", "Debug/polyglot_test.exe", "polyglot_test.exe"]
        .iter()
        .map(|p| build.join(p))
        .find(|p| p.is_file())
        .expect("the test program is built");
    let out = run(&mut Command::new(exe));
    assert_eq!(out, "polyglot [one, two]\n20! = 2432902008176640000\nok\n");
}

/// A generated package in a module of the language's test program (`lungo generate` owns the
/// package's directory).
fn generate_into(language: &str, components: &[&str], module: &Path, package: &str) -> PathBuf {
    let dist = distribution(components);
    std::fs::create_dir_all(module).unwrap();
    let out = module.join(package);
    run(Command::new(env!("CARGO_BIN_EXE_lungo"))
        .arg("--config")
        .arg(repo().join("compiler-tests/polyglot/lungo.toml"))
        .arg("generate")
        .arg(format!("--{language}_out={}", out.display()))
        .arg("--runtime-dir")
        .arg(&dist));
    out
}

#[test]
fn go_binding() {
    let module = root().join("go-module");
    let package = generate_into("go", &["go"], &module, "polyglot");
    let dist = distribution(&["go"]);
    std::fs::write(
        module.join("go.mod"),
        format!(
            "module example.com/polyglottest\n\ngo 1.22\n\nrequire github.com/jowharshamshiri/lungo-go v{v}\n\nreplace github.com/jowharshamshiri/lungo-go => {}\n",
            dist.join("go").display(),
            v = env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::copy(fixture("go").join("polyglot_test.go"), module.join("polyglot_test.go")).unwrap();
    let unformatted = run(Command::new("gofmt").arg("-l").arg(&package));
    assert!(unformatted.is_empty(), "the generated Go is not gofmt-formatted: {unformatted}");
    run(Command::new("go").arg("vet").arg("./...").current_dir(&module));
    let out = run(Command::new("go").args(["test", "-count=1", "-race", "./..."]).current_dir(&module));
    assert!(out.contains("ok  \texample.com/polyglottest"), "{out}");
}

#[test]
fn python_binding() {
    let dist = distribution(&["python"]);
    let package = generate("python", &["python"]);
    let venv = root().join("python-venv");
    if venv.exists() {
        std::fs::remove_dir_all(&venv).unwrap();
    }
    let python = std::env::var("PYTHON").unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into());
    run(Command::new(python).args(["-m", "venv"]).arg(&venv));
    let bin = venv.join(if cfg!(windows) { "Scripts" } else { "bin" });
    let py = bin.join(if cfg!(windows) { "python.exe" } else { "python" });
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet"]).arg(dist.join("python")));
    run(Command::new(&py).args(["-m", "unittest", "test_wire"]).current_dir(dist.join("python/tests")));
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet"]).arg(&package));
    let out = Command::new(&py)
        .args(["-m", "unittest", "-v", "test_polyglot"])
        .current_dir(fixture("python"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{text}");
    assert!(text.contains("Ran 10 tests") && text.trim_end().ends_with("OK"), "{text}");
}

#[cfg(target_os = "macos")]
#[test]
fn swift_binding() {
    let dist = distribution(&["swift"]);
    let package = generate("swift", &["swift"]);
    let tests = root().join("swift-tests");
    if tests.exists() {
        std::fs::remove_dir_all(&tests).unwrap();
    }
    std::fs::create_dir_all(tests.join("Tests/PolyglotTests")).unwrap();
    std::fs::create_dir_all(tests.join("Tests/PolyglotCAPITests")).unwrap();
    std::fs::copy(fixture("swift").join("PolyglotTests.swift"), tests.join("Tests/PolyglotTests/PolyglotTests.swift"))
        .unwrap();
    std::fs::copy(fixture("swift").join("CAPITests.m"), tests.join("Tests/PolyglotCAPITests/CAPITests.m")).unwrap();
    std::fs::write(
        tests.join("Package.swift"),
        format!(
            r#"// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "PolyglotE2E",
    platforms: [.macOS("12.0")],
    dependencies: [.package(path: {package:?}), .package(path: {support:?})],
    targets: [
        .testTarget(
            name: "PolyglotTests",
            dependencies: [.product(name: "Polyglot", package: "swift"), .product(name: "LungoKit", package: "lungo-swift")]
        ),
        .testTarget(
            name: "PolyglotCAPITests",
            dependencies: [.product(name: "PolyglotProgram", package: "swift")]
        ),
    ]
)
"#,
            package = package.display().to_string(),
            support = dist.join("lungo-swift").display().to_string(),
        ),
    )
    .unwrap();
    let out = Command::new("swift").arg("test").current_dir(&tests).output().unwrap();
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    assert!(out.status.success(), "{text}");
    assert!(!text.contains("warning:"), "the generated package builds with warnings:\n{text}");
    assert!(text.contains("Executed 10 tests, with 0 failures"), "the Swift tests ran:\n{text}");
    assert!(text.contains("Executed 3 tests, with 0 failures"), "the Objective-C tests ran:\n{text}");
}

#[test]
fn ts_binding() {
    let dist = distribution(&["ts"]);
    let package = generate("ts", &["ts"]);
    assert!(package.join("program.wasm").is_file(), "the WebAssembly module is linked");
    let project = root().join("ts-project");
    if project.exists() {
        std::fs::remove_dir_all(&project).unwrap();
    }
    std::fs::create_dir_all(&project).unwrap();
    for f in ["polyglot.test.js", "typecheck.ts"] {
        std::fs::copy(fixture("ts").join(f), project.join(f)).unwrap();
    }
    std::fs::write(
        project.join("package.json"),
        format!(
            r#"{{"name": "polyglot-e2e", "private": true, "type": "module", "dependencies": {{"polyglot": "file:{}", "lungo-ts": "file:{}"}}, "devDependencies": {{"typescript": "5.9.3"}}}}"#,
            package.display(),
            dist.join("ts").display()
        ),
    )
    .unwrap();
    std::fs::write(
        project.join("tsconfig.json"),
        r#"{"compilerOptions": {"target": "es2022", "module": "nodenext", "moduleResolution": "nodenext", "strict": true, "noEmit": true, "skipLibCheck": false}, "files": ["typecheck.ts"]}"#,
    )
    .unwrap();
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    run(Command::new(npm).args(["install", "--no-audit", "--no-fund", "--install-links"]).current_dir(&project));
    run(Command::new("node").args(["node_modules/typescript/bin/tsc", "-p", "."]).current_dir(&project));
    let out = run(Command::new("node").args(["--test", "polyglot.test.js"]).current_dir(&project));
    assert!(out.contains("# pass 10") && out.contains("# fail 0"), "{out}");
}

/// WebAssembly has no child processes: a program spawning one cannot be generated for it.
#[test]
fn webassembly_rejects_primitives_it_lacks() {
    let dist = distribution(&["ts"]);
    let out = root().join("ts-process");
    let result = Command::new(env!("CARGO_BIN_EXE_lungo"))
        .current_dir(root())
        .arg("--project")
        .arg(repo().join("compiler-tests/conformance"))
        .args(["--root", "Conformance.Process", "--name", "process", "generate"])
        .arg(format!("--ts_out={}", out.display()))
        .arg("--runtime-dir")
        .arg(&dist)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr);
    assert!(!result.status.success());
    assert!(stderr.contains("error[LNG0408]") && stderr.contains("lean_io_process_spawn"), "{stderr}");
    assert!(stderr.contains("WebAssembly has no child processes"), "{stderr}");
    assert!(!out.exists(), "nothing is published");
}
