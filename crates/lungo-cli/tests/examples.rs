//! The reference examples (`examples/*`) from the languages each is shown in: every example's
//! Rust crate is tested by its own tests; these generate its bindings for the other languages
//! with a local distribution and run the example's tests in each (`examples/<name>/<language>`).
//!
//! Each test requires its language's toolchain and fails when it is missing.

mod support;

use std::path::{Path, PathBuf};
use std::process::Command;
use support::{fresh, repo, run};

/// Where the tests work.
fn root() -> PathBuf {
    repo().join("target").join("examples-e2e")
}

fn distribution(components: &[&str]) -> PathBuf {
    support::distribution(&root(), components)
}

fn example(name: &str) -> PathBuf {
    repo().join("examples").join(name)
}

/// The example's tests in `language`: every file of `examples/<name>/<language>`.
fn copy_tests(name: &str, language: &str, into: &Path) {
    for entry in std::fs::read_dir(example(name).join(language)).unwrap() {
        let p = entry.unwrap().path();
        std::fs::copy(&p, into.join(p.file_name().unwrap())).unwrap();
    }
}

/// Generates example `name` (program `program`) for Go in a module of its own and runs its Go
/// tests there.
fn go(name: &str, program: &str) -> String {
    let dist = distribution(&["go"]);
    let module = fresh(&root().join(name).join("go"));
    let package = module.join(program);
    support::generate(&example(name).join("lungo.toml"), "go", &package, &dist);
    // A quoted go.mod path is a Go string literal, which a JSON string is too.
    let lungo_go = serde_json::to_string(&dist.join("go")).unwrap();
    std::fs::write(
        module.join("go.mod"),
        format!(
            "module example.com/{name}test\n\ngo 1.22\n\nrequire github.com/machinefabric/lungo-go v{v}\n\nreplace github.com/machinefabric/lungo-go => {lungo_go}\n",
            name = name.replace('-', ""),
            v = env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    copy_tests(name, "go", &module);
    let unformatted = run(Command::new("gofmt").arg("-l").arg(&module));
    assert!(unformatted.is_empty(), "not gofmt-formatted: {unformatted}");
    run(Command::new("go").arg("vet").arg("./...").current_dir(&module));
    run(Command::new("go").args(["test", "-count=1", "-race", "-v", "./..."]).current_dir(&module))
}

/// TEST0313: the sort example from Go
#[test]
fn test0313_the_sort_example_from_go() {
    let out = go("sort", "sorting");
    for t in ["Test0311_TheProvedSortSortsInGo", "Test0312_TheGoPackageCarriesTheSortsClaims"] {
        assert!(out.contains(&format!("--- PASS: {t}")), "{out}");
    }
}

/// Generates example `name` (program `program`) for TypeScript and runs its tests with Node.js.
fn ts(name: &str, program: &str) -> String {
    let dist = distribution(&["ts"]);
    let work = fresh(&root().join(name).join("ts"));
    let package = work.join(program);
    support::generate(&example(name).join("lungo.toml"), "ts", &package, &dist);
    let project = fresh(&work.join("project"));
    copy_tests(name, "ts", &project);
    let manifest = serde_json::json!({
        "name": format!("{name}-e2e"),
        "private": true,
        "type": "module",
        "dependencies": {
            program: format!("file:{}", package.display()),
            "lungo-ts": format!("file:{}", dist.join("ts").display()),
        },
    });
    std::fs::write(project.join("package.json"), manifest.to_string()).unwrap();
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    run(Command::new(npm).args(["install", "--no-audit", "--no-fund", "--install-links"]).current_dir(&project));
    run(Command::new("node").arg("--test").current_dir(&project))
}

/// Generates example `name` (program `program`) for C and builds its test program with CMake;
/// the program.
fn c(name: &str, program: &str) -> PathBuf {
    let dist = distribution(&["runtime"]);
    let work = fresh(&root().join(name).join("c"));
    let package = work.join(program);
    support::generate(&example(name).join("lungo.toml"), "c", &package, &dist);
    let build = work.join("build");
    run(Command::new("cmake")
        .arg("-S")
        .arg(example(name).join("c"))
        .arg("-B")
        .arg(&build)
        .arg(format!("-DEXAMPLE_PACKAGE={}", package.display()))
        .arg("-DCMAKE_BUILD_TYPE=Debug"));
    run(Command::new("cmake").arg("--build").arg(&build).args(["--config", "Debug", "--parallel"]));
    let exe = format!("{name}_test");
    [exe.clone(), format!("Debug/{exe}.exe"), format!("{exe}.exe")]
        .iter()
        .map(|p| build.join(p))
        .find(|p| p.is_file())
        .expect("the test program is built")
}

/// TEST0319: the codec example from TypeScript and C
#[test]
fn test0319_the_codec_example_from_typescript_and_c() {
    let out = ts("codec", "varint");
    assert!(out.contains("# pass 1") && out.contains("# fail 0"), "{out}");
    assert_eq!(run(&mut Command::new(c("codec", "varint"))), "ok\n");
}

/// TEST0322: deadlines on a host's clock from C, and on a clock breaking the assumption
#[test]
fn test0322_deadlines_on_a_hosts_clock_from_c_and_on_a_clock_breaking_the_assumption() {
    let exe = c("clock", "timing");
    assert_eq!(
        run(&mut Command::new(&exe)),
        "round trip of 7 s: 7 s; deadline in 5 s, after 2.5 s: 2 s left\nassumes Timing.Ticks: yes\n"
    );
    // The claims are proved, and the host breaks what they assume: the round trip they promise
    // does not happen.
    assert_eq!(
        run(Command::new(&exe).arg("lawless")),
        "round trip of 7 s: 0 s; deadline in 5 s, after 2.5 s: 0 s left\nassumes Timing.Ticks: yes\n"
    );
}

/// Generates example `name` (program `program`, Swift module `module`) for Swift and runs its
/// XCTest tests with SwiftPM.
#[cfg(target_os = "macos")]
fn swift(name: &str, module: &str) -> String {
    let dist = distribution(&["swift"]);
    let work = fresh(&root().join(name).join("swift"));
    let package = work.join("package");
    support::generate(&example(name).join("lungo.toml"), "swift", &package, &dist);
    let tests = work.join("tests");
    let target = format!("{module}Tests");
    std::fs::create_dir_all(tests.join("Tests").join(&target)).unwrap();
    copy_tests(name, "swift", &tests.join("Tests").join(&target));
    std::fs::write(
        tests.join("Package.swift"),
        format!(
            r#"// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "{module}E2E",
    platforms: [.macOS("12.0")],
    dependencies: [.package(path: {package:?}), .package(path: {support:?})],
    targets: [
        .testTarget(
            name: "{target}",
            dependencies: [.product(name: "{module}", package: "package"), .product(name: "LungoKit", package: "lungo-swift")]
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
    text
}

/// TEST0328: the ledger example from Go
#[test]
fn test0328_the_ledger_example_from_go() {
    let out = go("ledger", "ledger");
    assert!(out.contains("--- PASS: Test0326_TheProvedLedgerFromGo"), "{out}");
}

/// TEST0329: the ledger example from Swift
#[cfg(target_os = "macos")]
#[test]
fn test0329_the_ledger_example_from_swift() {
    let out = swift("ledger", "Ledger");
    assert!(out.contains("Executed 1 test, with 0 failures"), "{out}");
}

/// Generates example `name` (program `program`) for Python, installs it in a virtual environment
/// with lungo-py, and runs its tests with `unittest`.
fn python(name: &str, program: &str) -> String {
    let dist = distribution(&["python"]);
    let work = fresh(&root().join(name).join("python"));
    let package = work.join(program);
    support::generate(&example(name).join("lungo.toml"), "python", &package, &dist);
    let venv = work.join("venv");
    let python = std::env::var("PYTHON").unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into());
    run(Command::new(python).args(["-m", "venv"]).arg(&venv));
    let py = venv.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" });
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet"]).arg(dist.join("python")));
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet"]).arg(&package));
    let tests = fresh(&work.join("tests"));
    copy_tests(name, "python", &tests);
    let out = Command::new(&py).args(["-m", "unittest", "-v"]).current_dir(&tests).output().unwrap();
    let text = String::from_utf8_lossy(&out.stderr).into_owned();
    assert!(out.status.success(), "{text}");
    text
}

/// TEST0335: the oracle-monitor example from Python
#[test]
fn test0335_the_oracle_monitor_example_from_python() {
    let out = python("oracle-monitor", "access");
    assert!(out.contains("Ran 2 tests") && out.trim_end().ends_with("OK"), "{out}");
}

/// TEST0340: the async-store example from TypeScript and Python
#[test]
fn test0340_the_async_store_example_from_typescript_and_python() {
    let out = ts("async-store", "store");
    assert!(out.contains("# pass 1") && out.contains("# fail 0"), "{out}");
    let out = python("async-store", "store");
    assert!(out.contains("Ran 1 test") && out.trim_end().ends_with("OK"), "{out}");
}

/// TEST0344: the semantic-model example from Go, composed with Acme's package
#[test]
fn test0344_the_semantic_model_example_from_go_composed_with_acmes_package() {
    let out = go("semantic-model", "inventory");
    assert!(out.contains("--- PASS: Test0343_TheInventoryAndAcmesClaimFromGo"), "{out}");
    let dist = distribution(&["go"]);
    let work = fresh(&root().join("semantic-model").join("compose"));
    let inventory = work.join("inventory");
    support::generate(&example("semantic-model").join("lungo.toml"), "c", &inventory, &dist);
    let acme = work.join("acme");
    support::generate(&example("semantic-model").join("acme/lungo.toml"), "c", &acme, &dist);
    let compose = |other: &Path| {
        Command::new(env!("CARGO_BIN_EXE_lungo"))
            .args(["assurance", "--compose"])
            .arg(inventory.join("assurance.json"))
            .arg("--compose")
            .arg(other.join("assurance.json"))
            .output()
            .unwrap()
    };
    // Both describe Acme.Linear, from the package that declares it, the same way.
    let out = compose(&acme);
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(String::from_utf8_lossy(&out.stdout).contains("2 documents agree"));
    // Acme changes its model: the inventory's claim was proved against the old one.
    let changed = work.join("acme-changed");
    let source = std::fs::read_to_string(example("semantic-model").join("acme/Acme.lean")).unwrap();
    let edited = source.replace("fun a => k * size a + k", "fun a => k * size a");
    assert_ne!(source, edited, "the model's definition changed: update this test");
    std::fs::create_dir_all(&changed).unwrap();
    for f in ["lean-toolchain", "lungo.toml"] {
        std::fs::copy(example("semantic-model").join("acme").join(f), changed.join(f)).unwrap();
    }
    std::fs::write(changed.join("Acme.lean"), edited).unwrap();
    let lungo_lib = serde_json::to_string(&repo().join("lean")).unwrap();
    std::fs::write(
        changed.join("lakefile.toml"),
        format!(
            "name = \"acme\"\nversion = \"0.1.0\"\ndefaultTargets = [\"Acme\"]\n\n[[require]]\nname = \"lungo\"\npath = {lungo_lib}\n\n[[lean_lib]]\nname = \"Acme\"\n"
        ),
    )
    .unwrap();
    std::fs::write(
        changed.join("lake-manifest.json"),
        format!(
            r#"{{"version": "1.2.0", "packagesDir": ".lake/packages", "packages": [{{"type": "path", "scope": "", "name": "lungo", "manifestFile": "lake-manifest.json", "inherited": false, "dir": {lungo_lib}, "configFile": "lakefile.toml"}}], "name": "acme", "lakeDir": ".lake", "fixedToolchain": false}}"#
        ),
    )
    .unwrap();
    let changed_out = work.join("acme-changed-out");
    support::generate(&changed.join("lungo.toml"), "c", &changed_out, &dist);
    let out = compose(&changed_out);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3), "{stderr}");
    assert!(stderr.contains("error[LNG0708]") && stderr.contains("Acme.Linear"), "{stderr}");
}
