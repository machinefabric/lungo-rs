//! End-to-end tests of every language binding: the polyglot fixture
//! (`compiler-tests/polyglot`) is generated for each language with a local distribution built
//! from this repository, the generated package is built with the language's own toolchain, and
//! the language's test program makes the same assertions on it.
//!
//! Each test requires its language's toolchain (CMake and a C compiler, Go, Python, Swift,
//! Node.js) and fails when it is missing.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use support::{repo, run};

/// Where the tests work: the fixture's distribution, generated packages and builds.
fn root() -> PathBuf {
    repo().join("target").join("polyglot-e2e")
}

/// The local distribution with `components`.
fn distribution(components: &[&str]) -> PathBuf {
    support::distribution(&root(), components)
}

/// Generates the fixture for `language` with the distribution components `components`.
fn generate(language: &str, components: &[&str]) -> PathBuf {
    let out = root().join(language);
    support::generate(&repo().join("compiler-tests/polyglot/lungo.toml"), language, &out, &distribution(components));
    out
}

fn fixture(language: &str) -> PathBuf {
    repo().join("compiler-tests/polyglot").join(language)
}

/// TEST0111: c binding
#[test]
fn test0111_c_binding() {
    let package = generate("c", &["runtime"]);
    let build = root().join("c-build");
    // A fresh build: a CMake cache records the absolute paths it was configured with.
    if build.exists() {
        std::fs::remove_dir_all(&build).unwrap();
    }
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
    let out = run(&mut Command::new(&exe));
    assert_eq!(out, "polyglot [one, two]\n20! = 2432902008176640000\nok\n");
    // TEST0297: without the scaler, the program's first call ends the process, naming it.
    let out = Command::new(&exe).arg("without-facilities").output().unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !out.status.success()
            && stderr.contains("the host does not provide the facility polyglot.scaler: its operation hostScale")
            && !stderr.contains("the call returned"),
        "{}: {stderr}",
        out.status
    );
}

/// A generated package in a module of the language's test program (`lungo generate` owns the
/// package's directory).
fn generate_into(language: &str, components: &[&str], module: &Path, package: &str) -> PathBuf {
    std::fs::create_dir_all(module).unwrap();
    let out = module.join(package);
    support::generate(&repo().join("compiler-tests/polyglot/lungo.toml"), language, &out, &distribution(components));
    out
}

/// TEST0112: go binding
#[test]
fn test0112_go_binding() {
    let module = root().join("go-module");
    let package = generate_into("go", &["go"], &module, "polyglot");
    let dist = distribution(&["go"]);
    // A quoted go.mod path is a Go string literal, which a JSON string is too.
    let support = serde_json::to_string(&dist.join("go")).unwrap();
    std::fs::write(
        module.join("go.mod"),
        format!(
            "module example.com/polyglottest\n\ngo 1.22\n\nrequire github.com/machinefabric/lungo-go v{v}\n\nreplace github.com/machinefabric/lungo-go => {support}\n",
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

/// TEST0113: python binding
#[test]
fn test0113_python_binding() {
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
    let tests = root().join("python-tests");
    if tests.exists() {
        std::fs::remove_dir_all(&tests).unwrap();
    }
    std::fs::create_dir_all(&tests).unwrap();
    std::fs::copy(fixture("python").join("test_polyglot.py"), tests.join("test_polyglot.py")).unwrap();
    let unittest = |label: &str| {
        let out =
            Command::new(&py).args(["-m", "unittest", "-v", "test_polyglot"]).current_dir(&tests).output().unwrap();
        let text = String::from_utf8_lossy(&out.stderr);
        assert!(out.status.success(), "{label}: {text}");
        assert!(text.contains("Ran 18 tests") && text.trim_end().ends_with("OK"), "{label}: {text}");
    };
    unittest("installed");
    // Installed editable, the sources are imported from the package's own directory
    // while the compiled program is installed in site-packages: the package finds its
    // program through its `__path__`, which names both.
    run(Command::new(&py).args(["-m", "pip", "uninstall", "--yes", "--quiet", "polyglot"]));
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet", "--editable"]).arg(&package));
    unittest("editable");
}

/// TEST0114: swift binding
#[cfg(target_os = "macos")]
#[test]
fn test0114_swift_binding() {
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
    let swift_test = |args: &[&str], env: &[(&str, &str)]| {
        let out = Command::new("swift")
            .arg("test")
            .args(args)
            .envs(env.iter().copied())
            .current_dir(&tests)
            .output()
            .unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        assert!(out.status.success(), "{text}");
        text
    };
    // The test of a missing facility runs alone: every other test installs the facilities.
    let text = swift_test(&["--skip", "MissingFacilityTests"], &[]);
    assert!(!text.contains("warning:"), "the generated package builds with warnings:\n{text}");
    assert!(text.contains("Executed 17 tests, with 0 failures"), "the Swift tests ran:\n{text}");
    assert!(text.contains("Executed 3 tests, with 0 failures"), "the Objective-C tests ran:\n{text}");
    let text = swift_test(&["--filter", "MissingFacilityTests"], &[("LUNGO_POLYGLOT_WITHOUT_FACILITIES", "1")]);
    assert!(text.contains("Executed 1 test, with 0 failures") && !text.contains("skipped"), "TEST0297 ran:\n{text}");
}

/// TEST0115: ts binding
#[test]
fn test0115_ts_binding() {
    let dist = distribution(&["ts"]);
    let package = generate("ts", &["ts"]);
    assert!(package.join("program.wasm").is_file(), "the WebAssembly module is linked");

    // `program.wasm` is a platform product: `--verify` does not compare it, and
    // `--link` links it again for this machine — the same bytes `generate` linked
    // here — into an output whose sources are current, and refuses one whose
    // sources are not, leaving the module as it is.
    let lungo = |mode: &str| {
        let out = Command::new(env!("CARGO_BIN_EXE_lungo"))
            .arg("--config")
            .arg(repo().join("compiler-tests/polyglot/lungo.toml"))
            .arg("generate")
            .arg(format!("--ts_out={}", package.display()))
            .arg("--runtime-dir")
            .arg(&dist)
            .arg(mode)
            .output()
            .unwrap();
        (
            out.status.success(),
            format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        )
    };
    let module = package.join("program.wasm");
    let linked = std::fs::read(&module).unwrap();
    std::fs::write(&module, b"linked elsewhere").unwrap();
    let (ok, text) = lungo("--verify");
    assert!(ok && text.contains("verified: ts"), "--verify compared the platform product:\n{text}");
    let (ok, text) = lungo("--link");
    assert!(ok && text.contains("linked: ts"), "{text}");
    assert!(std::fs::read(&module).unwrap() == linked, "--link did not link the module generate linked");
    let index = package.join("index.js");
    let source = std::fs::read_to_string(&index).unwrap();
    std::fs::write(&index, format!("{source}// edited by hand\n")).unwrap();
    std::fs::write(&module, b"linked elsewhere").unwrap();
    let (ok, text) = lungo("--link");
    assert!(!ok && text.contains("LNG0110") && text.contains("changed: index.js"), "{text}");
    assert_eq!(std::fs::read(&module).unwrap(), b"linked elsewhere", "--link linked over stale sources");
    std::fs::write(&index, source).unwrap();
    std::fs::write(&module, &linked).unwrap();
    let project = root().join("ts-project");
    if project.exists() {
        std::fs::remove_dir_all(&project).unwrap();
    }
    std::fs::create_dir_all(&project).unwrap();
    for f in ["polyglot.test.js", "typecheck.ts"] {
        std::fs::copy(fixture("ts").join(f), project.join(f)).unwrap();
    }
    let manifest = serde_json::json!({
        "name": "polyglot-e2e",
        "private": true,
        "type": "module",
        "dependencies": {
            "polyglot": format!("file:{}", package.display()),
            "lungo-ts": format!("file:{}", dist.join("ts").display()),
        },
        "devDependencies": { "typescript": "5.9.3" },
    });
    std::fs::write(project.join("package.json"), manifest.to_string()).unwrap();
    std::fs::write(
        project.join("tsconfig.json"),
        r#"{"compilerOptions": {"target": "es2022", "module": "nodenext", "moduleResolution": "nodenext", "strict": true, "noEmit": true, "skipLibCheck": false}, "files": ["typecheck.ts"]}"#,
    )
    .unwrap();
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    run(Command::new(npm).args(["install", "--no-audit", "--no-fund", "--install-links"]).current_dir(&project));
    run(Command::new("node").args(["node_modules/typescript/bin/tsc", "-p", "."]).current_dir(&project));
    let out = run(Command::new("node").args(["--test", "polyglot.test.js"]).current_dir(&project));
    assert!(out.contains("# pass 18") && out.contains("# fail 0"), "{out}");
}

/// TEST0116: WebAssembly has no child processes: a program spawning one cannot be generated for it.
#[test]
fn test0116_webassembly_rejects_primitives_it_lacks() {
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
