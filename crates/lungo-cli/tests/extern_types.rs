//! Types shared between generated packages, in every language: the extern-types fixture
//! (`compiler-tests/extern`) generates `provider` as a package of its own and `consumer`, whose
//! functions take and return `provider`'s types, embedded in a host package that takes those
//! types from `provider`'s package. The host's program then passes values made by one program to
//! the other; and a `provider` generated from another definition of its types must be refused
//! before anything runs, since `consumer`'s compiled code would read its values at the wrong
//! layout.
//!
//! Each test requires its language's toolchain (Go, Python, Node.js, Swift, CMake and a C
//! compiler) and fails when it is missing.

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Mutex, OnceLock};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
}

/// Where the tests work.
fn root() -> PathBuf {
    repo().join("target").join("extern-e2e")
}

fn fixture() -> PathBuf {
    repo().join("compiler-tests/extern")
}

#[track_caller]
fn run(cmd: &mut Command) -> String {
    let shown = format!("{cmd:?}");
    let out = cmd.output().unwrap_or_else(|e| panic!("cannot run {shown}: {e} (is the toolchain installed?)"));
    assert!(
        out.status.success(),
        "{shown} failed ({}):\n{}\n{}",
        out.status,
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn text(out: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))
}

/// A fresh directory.
fn fresh(dir: &Path) -> PathBuf {
    if dir.exists() {
        std::fs::remove_dir_all(dir).unwrap();
    }
    std::fs::create_dir_all(dir).unwrap();
    dir.to_path_buf()
}

/// The local distribution with `components`, built once per component.
fn distribution(components: &[&str]) -> PathBuf {
    static BUILT: OnceLock<Mutex<Vec<String>>> = OnceLock::new();
    let mut built = BUILT.get_or_init(Default::default).lock().unwrap_or_else(|p| p.into_inner());
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

/// `provider` modified: `Pos` gains a field and `Pair` another, so both layouts change.
fn changed_provider(work: &Path) -> PathBuf {
    let dir = fresh(&work.join("provider-changed"));
    for f in ["lakefile.toml", "lean-toolchain", "lake-manifest.json", "Provider.lean"] {
        std::fs::copy(fixture().join("provider").join(f), dir.join(f)).unwrap();
    }
    let source = std::fs::read_to_string(dir.join("Provider.lean")).unwrap();
    let changed = source
        .replace("  n : Nat\n  pos : 0 < n", "  n : Nat\n  tag : String\n  pos : 0 < n")
        .replace("some ⟨n, h⟩", "some ⟨n, \"\", h⟩")
        .replace("  count : Nat\n  label : String", "  count : Nat\n  label : String\n  weight : Nat")
        .replace("⟨count, label⟩", "⟨count, label, 0⟩");
    assert_ne!(source, changed, "the fixture's definitions changed shape: update this test");
    std::fs::write(dir.join("Provider.lean"), changed).unwrap();
    dir
}

/// Runs `lungo generate` with the configuration `config` (a `lungo.toml` written for it).
fn generate(work: &Path, name: &str, config: &str, dist: &Path) -> Output {
    let file = work.join(format!("{name}.toml"));
    std::fs::write(&file, config).unwrap();
    Command::new(env!("CARGO_BIN_EXE_lungo"))
        .arg("--config")
        .arg(&file)
        .arg("generate")
        .arg("--runtime-dir")
        .arg(dist)
        .output()
        .unwrap()
}

#[track_caller]
fn generated(out: Output) {
    assert!(out.status.success(), "lungo generate failed:\n{}", text(&out));
}

/// A TOML string.
fn t(p: &Path) -> String {
    serde_json::to_string(&p.to_string_lossy()).unwrap()
}

/// The extern-types tables of `language` mapping `provider`'s two types.
fn extern_tables(language: &str, package: &str, pos: &str, pair: &str) -> String {
    format!(
        "[{language}.extern-types.\"Provider.Pos\"]\npackage = {package:?}\nname = {pos:?}\n[{language}.extern-types.\"Provider.Pair\"]\npackage = {package:?}\nname = {pair:?}\n"
    )
}

#[test]
fn go_programs_share_values_and_refuse_another_layout() {
    let work = fresh(&root().join("go"));
    let dist = distribution(&["go"]);
    let module = fresh(&work.join("module"));
    let provider = |project: &Path| {
        format!("project = {}\n[go]\nout = {}\n", t(project), t(&module.join("provider")))
    };
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    generated(generate(
        &work,
        "consumer",
        &format!(
            "project = {}\n[go]\nout = {}\n{}",
            t(&fixture().join("consumer")),
            t(&module.join("consumer")),
            extern_tables("go", "example.com/ext/provider", "Pos", "Pair")
        ),
        &dist,
    ));
    std::fs::write(
        module.join("go.mod"),
        format!(
            "module example.com/ext\n\ngo 1.22\n\nrequire github.com/machinefabric/lungo-go v{v}\n\nreplace github.com/machinefabric/lungo-go => {}\n",
            serde_json::to_string(&dist.join("go")).unwrap(),
            v = env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
    std::fs::copy(fixture().join("go/extern_test.go"), module.join("extern_test.go")).unwrap();
    let test = || Command::new("go").args(["test", "-count=1", "./..."]).current_dir(&module).output().unwrap();
    let passed = test();
    assert!(passed.status.success(), "{}", text(&passed));
    assert!(text(&passed).contains("ok  \texample.com/ext"), "{}", text(&passed));

    // A provider generated from another definition: the consumer refuses to start.
    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    let refused = test();
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("Provider.Pos of example.com/ext/provider has another layout"), "{}", text(&refused));
}

#[test]
fn python_programs_share_values_and_refuse_another_layout() {
    let work = fresh(&root().join("python"));
    let dist = distribution(&["python"]);
    let provider_pkg = work.join("provider");
    let host = fresh(&work.join("host"));
    let provider = |project: &Path| format!("project = {}\n[python]\nout = {}\n", t(project), t(&provider_pkg));
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    // The consumer is a module of the host's package.
    generated(generate(
        &work,
        "consumer",
        &format!(
            "project = {}\n[python]\nout = {}\noptions = {{ embed = \"host.consumer\" }}\n{}",
            t(&fixture().join("consumer")),
            t(&host.join("src/host/consumer")),
            extern_tables("python", "provider", "Pos", "Pair")
        ),
        &dist,
    ));
    let file_url = |p: &Path| format!("file://{}", p.display());
    std::fs::write(host.join("src/host/__init__.py"), "").unwrap();
    std::fs::write(
        host.join("pyproject.toml"),
        format!(
            "[build-system]\nrequires = [\"scikit-build-core>=0.10\", \"lungo-py @ {lungo}\"]\nbuild-backend = \"scikit_build_core.build\"\n\n[project]\nname = \"host\"\nversion = \"0.1.0\"\ndependencies = [\"lungo-py @ {lungo}\"]\n\n[tool.scikit-build]\nwheel.packages = [\"src/host\"]\nwheel.py-api = \"py3\"\n",
            lungo = file_url(&dist.join("python"))
        ),
    )
    .unwrap();
    std::fs::write(
        host.join("CMakeLists.txt"),
        "cmake_minimum_required(VERSION 3.20)\nproject(host LANGUAGES C)\nadd_subdirectory(src/host/consumer)\n",
    )
    .unwrap();
    let venv = work.join("venv");
    let python = std::env::var("PYTHON").unwrap_or_else(|_| if cfg!(windows) { "python" } else { "python3" }.into());
    run(Command::new(python).args(["-m", "venv"]).arg(&venv));
    let py = venv.join(if cfg!(windows) { "Scripts/python.exe" } else { "bin/python" });
    let install = |pkg: &Path| run(Command::new(&py).args(["-m", "pip", "install", "--quiet", "--force-reinstall", "--no-deps"]).arg(pkg));
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet"]).arg(dist.join("python")));
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet", "scikit-build-core>=0.10"]));
    install(&provider_pkg);
    // Its dependencies are installed above; built here, against the scikit-build-core installed.
    run(Command::new(&py).args(["-m", "pip", "install", "--quiet", "--no-build-isolation", "--no-deps"]).arg(&host));
    let script = fixture().join("python/test_extern.py");
    let test = || Command::new(&py).arg(&script).current_dir(&work).output().unwrap();
    let passed = test();
    assert!(passed.status.success() && text(&passed).contains("ok"), "{}", text(&passed));

    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    install(&provider_pkg);
    let refused = test();
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("ImportError"), "{}", text(&refused));
    assert!(text(&refused).contains("of provider has another layout"), "{}", text(&refused));
}

#[test]
fn typescript_programs_share_data_but_not_handles() {
    let work = fresh(&root().join("ts"));
    let dist = distribution(&["ts"]);
    let provider_pkg = work.join("provider");
    let host = fresh(&work.join("host"));
    let provider = |project: &Path| {
        format!("project = {}\n[ts]\nout = {}\noptions = {{ package = \"provider\" }}\n", t(project), t(&provider_pkg))
    };
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    let consumer = |tables: &str| {
        format!(
            "project = {}\n[ts]\nout = {}\noptions = {{ embed = \"true\" }}\n{tables}",
            t(&fixture().join("consumer")),
            t(&host.join("consumer"))
        )
    };
    // `Pos` is held by handle: a handle of one WebAssembly instance names nothing in another.
    let refused = generate(&work, "consumer", &consumer(&extern_tables("ts", "provider", "Pos", "Pair")), &dist);
    assert!(!refused.status.success());
    assert!(text(&refused).contains("`Provider.Pos` holds Lean values by handle"), "{}", text(&refused));
    // `Pair` is data, which passes by value.
    let pair_only = "[ts.extern-types.\"Provider.Pair\"]\npackage = \"provider\"\nname = \"Pair\"\n";
    generated(generate(&work, "consumer", &consumer(pair_only), &dist));
    std::fs::write(
        host.join("package.json"),
        json!({
            "name": "host",
            "private": true,
            "type": "module",
            "dependencies": {
                "provider": format!("file:{}", provider_pkg.display()),
                "lungo-ts": format!("file:{}", dist.join("ts").display()),
            },
        })
        .to_string(),
    )
    .unwrap();
    std::fs::copy(fixture().join("ts/extern.test.js"), host.join("extern.test.js")).unwrap();
    let npm = if cfg!(windows) { "npm.cmd" } else { "npm" };
    // From scratch each time: npm keeps an installed copy of a `file:` package of the same version.
    let install = || {
        for stale in ["node_modules", "package-lock.json"] {
            let p = host.join(stale);
            if p.is_dir() {
                std::fs::remove_dir_all(&p).unwrap();
            } else if p.exists() {
                std::fs::remove_file(&p).unwrap();
            }
        }
        run(Command::new(npm).args(["install", "--no-audit", "--no-fund", "--install-links"]).current_dir(&host))
    };
    install();
    let test = || Command::new("node").args(["--test", "extern.test.js"]).current_dir(&host).output().unwrap();
    let passed = test();
    assert!(passed.status.success() && text(&passed).contains("# fail 0"), "{}", text(&passed));

    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    install();
    let refused = test();
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("Provider.Pair of provider has another layout"), "{}", text(&refused));
}

#[cfg(target_os = "macos")]
#[test]
fn swift_programs_share_values_and_refuse_another_layout() {
    let work = fresh(&root().join("swift"));
    let dist = distribution(&["swift"]);
    let provider_pkg = work.join("provider-swift");
    let host = fresh(&work.join("host"));
    let provider = |project: &Path| {
        format!("project = {}\n[swift]\nout = {}\noptions = {{ module = \"Provider\" }}\n", t(project), t(&provider_pkg))
    };
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    generated(generate(
        &work,
        "consumer",
        &format!(
            "project = {}\n[swift]\nout = {}\noptions = {{ module = \"Consumer\", embed = \"true\" }}\n{}",
            t(&fixture().join("consumer")),
            t(&host.join("Generated")),
            extern_tables("swift", "Provider", "Pos", "Pair")
        ),
        &dist,
    ));
    std::fs::create_dir_all(host.join("Tests/HostTests")).unwrap();
    std::fs::copy(fixture().join("swift/ExternTests.swift"), host.join("Tests/HostTests/ExternTests.swift")).unwrap();
    std::fs::write(
        host.join("Package.swift"),
        format!(
            r#"// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Host",
    platforms: [.macOS("12.0")],
    dependencies: [.package(path: {provider:?}), .package(path: {support:?})],
    targets: [
        .target(
            name: "ConsumerProgram",
            dependencies: [.product(name: "LungoKit", package: "lungo-swift")],
            path: "Generated/ConsumerProgram",
            cSettings: [.headerSearchPath("program")]
        ),
        .target(
            name: "Consumer",
            dependencies: [
                "ConsumerProgram",
                .product(name: "LungoKit", package: "lungo-swift"),
                .product(name: "Provider", package: "provider-swift"),
            ],
            path: "Generated/Consumer"
        ),
        .testTarget(
            name: "HostTests",
            dependencies: ["Consumer", .product(name: "Provider", package: "provider-swift"), .product(name: "LungoKit", package: "lungo-swift")]
        ),
    ]
)
"#,
            provider = provider_pkg.display().to_string(),
            support = dist.join("lungo-swift").display().to_string(),
        ),
    )
    .unwrap();
    let test = || Command::new("swift").arg("test").current_dir(&host).output().unwrap();
    let passed = test();
    assert!(passed.status.success() && text(&passed).contains("Executed 1 test, with 0 failures"), "{}", text(&passed));

    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    let refused = test();
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("Provider.Pos of Provider has another layout"), "{}", text(&refused));
}

#[test]
fn c_programs_share_values_and_refuse_another_layout() {
    let work = fresh(&root().join("c"));
    let dist = distribution(&["runtime"]);
    let provider_pkg = work.join("provider");
    let consumer_pkg = work.join("consumer");
    let provider = |project: &Path| format!("project = {}\n[c]\nout = {}\n", t(project), t(&provider_pkg));
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    generated(generate(
        &work,
        "consumer",
        &format!(
            "project = {}\n[c]\nout = {}\noptions = {{ embed = \"true\" }}\n{}",
            t(&fixture().join("consumer")),
            t(&consumer_pkg),
            extern_tables("c", "provider.h", "provider_pos", "provider_pair")
        ),
        &dist,
    ));
    let build_and_run = |build: &Path| -> Output {
        let build = fresh(build);
        run(Command::new("cmake")
            .arg("-S")
            .arg(fixture().join("c"))
            .arg("-B")
            .arg(&build)
            .arg(format!("-DLUNGO_VERSION={}", env!("CARGO_PKG_VERSION")))
            .arg(format!("-DLUNGO_RUNTIME={}", dist.join("runtime").display()))
            .arg(format!("-DPROVIDER_PACKAGE={}", provider_pkg.display()))
            .arg(format!("-DCONSUMER_PACKAGE={}", consumer_pkg.display()))
            .arg("-DCMAKE_BUILD_TYPE=Debug"));
        run(Command::new("cmake").arg("--build").arg(&build).args(["--config", "Debug", "--parallel"]));
        let exe = ["extern_test", "Debug/extern_test.exe", "extern_test.exe"]
            .iter()
            .map(|p| build.join(p))
            .find(|p| p.is_file())
            .expect("the test program is built");
        Command::new(exe).output().unwrap()
    };
    let passed = build_and_run(&work.join("build"));
    assert!(passed.status.success(), "{}", text(&passed));
    assert_eq!(String::from_utf8_lossy(&passed.stdout).trim(), "ok");

    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    let refused = build_and_run(&work.join("build-changed"));
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("Provider.Pos of provider.h has another layout"), "{}", text(&refused));
}

#[test]
fn rust_modules_share_values_and_refuse_another_layout_when_compiled() {
    let work = fresh(&root().join("rust"));
    let krate = fresh(&work.join("host"));
    let generated_dir = krate.join("gen");
    let provider = |project: &Path| {
        format!("project = {}\n[rust]\nout-dir = {}\n", t(project), t(&generated_dir))
    };
    let dist = work.join("no-distribution");
    generated(generate(&work, "provider", &provider(&fixture().join("provider")), &dist));
    generated(generate(
        &work,
        "consumer",
        &format!(
            "project = {}\n[rust]\nout-dir = {}\n[rust.extern-types]\n\"Provider.Pos\" = \"::lungo::LeanValue<crate::provider::__opaque::Pos>\"\n\"Provider.Pair\" = \"crate::provider::Pair\"\n",
            t(&fixture().join("consumer")),
            t(&generated_dir)
        ),
        &dist,
    ));
    std::fs::write(
        krate.join("Cargo.toml"),
        format!(
            "[package]\nname = \"host\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[dependencies]\nlungo = {{ path = {} }}\n\n[workspace]\n",
            t(&repo().join("crates/lungo"))
        ),
    )
    .unwrap();
    std::fs::create_dir_all(krate.join("src")).unwrap();
    std::fs::write(
        krate.join("src/lib.rs"),
        "pub mod provider {\n    include!(\"../gen/provider/provider.rs\");\n}\npub mod consumer {\n    include!(\"../gen/consumer/consumer.rs\");\n}\n",
    )
    .unwrap();
    std::fs::create_dir_all(krate.join("tests")).unwrap();
    std::fs::copy(fixture().join("rust/extern.rs"), krate.join("tests/extern.rs")).unwrap();
    let test = || {
        Command::new(env!("CARGO"))
            .arg("test")
            .current_dir(&krate)
            .env("CARGO_TARGET_DIR", work.join("target"))
            .output()
            .unwrap()
    };
    let passed = test();
    assert!(passed.status.success(), "{}", text(&passed));

    // The check is a constant: a provider of another layout does not compile with the consumer.
    generated(generate(&work, "provider", &provider(&changed_provider(&work)), &dist));
    let refused = test();
    assert!(!refused.status.success(), "a provider of another layout was accepted:\n{}", text(&refused));
    assert!(text(&refused).contains("has a different Lean layout"), "{}", text(&refused));
}
