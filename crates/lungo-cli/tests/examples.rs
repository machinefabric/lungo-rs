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

/// Generates example `name` (program `program`) for C, builds its test program with CMake, and
/// runs it.
fn c(name: &str, program: &str) -> String {
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
    let exe = [exe.clone(), format!("Debug/{exe}.exe"), format!("{exe}.exe")]
        .iter()
        .map(|p| build.join(p))
        .find(|p| p.is_file())
        .expect("the test program is built");
    run(&mut Command::new(exe))
}

/// TEST0319: the codec example from TypeScript and C
#[test]
fn test0319_the_codec_example_from_typescript_and_c() {
    let out = ts("codec", "varint");
    assert!(out.contains("# pass 1") && out.contains("# fail 0"), "{out}");
    assert_eq!(c("codec", "varint"), "ok\n");
}
