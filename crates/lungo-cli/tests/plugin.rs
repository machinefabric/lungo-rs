//! The plugin protocol: `--<name>_out` runs `lungo-gen-<name>` from `PATH` with the request on its
//! standard input, publishes the files of its response, and reports its failures as LNG0504.
//!
//! This test program is also the plugin: run as `lungo-gen-echo`, it reads a `GenerateRequest`
//! and answers a `GenerateResponse`, writing `functions.txt` (each exported function's Lean name,
//! entry point and type) and `request.json` (the request). Its options: `fail` (report an
//! error), `garbage` (write a malformed response), `exit` (exit with this status).

use lungo_build::codegen::plugin::{GenerateRequest, GenerateResponse, PROTOCOL_VERSION};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let exe = std::env::current_exe().unwrap();
    if exe.file_stem().is_some_and(|s| s == "lungo-gen-echo") {
        return plugin();
    }
    let tests: [(&str, fn()); 2] = [
        (
            "a_plugin_receives_the_request_and_its_files_are_published",
            a_plugin_receives_the_request_and_its_files_are_published,
        ),
        (
            "plugin_failures_are_reported_and_nothing_is_published",
            plugin_failures_are_reported_and_nothing_is_published,
        ),
    ];
    for (name, test) in tests {
        test();
        println!("test {name} ... ok");
    }
}

/// The plugin.
fn plugin() {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("the request is readable");
    let request: GenerateRequest = serde_json::from_str(&input).expect("the request is a GenerateRequest");
    let mut response = GenerateResponse::default();
    if request.protocol_version != PROTOCOL_VERSION {
        response.errors.push(format!("protocol {} is not {PROTOCOL_VERSION}", request.protocol_version));
    }
    for key in request.options.keys() {
        if !["fail", "garbage", "exit"].contains(&key.as_str()) {
            response.errors.push(format!("unknown option `{key}`"));
        }
    }
    if let Some(code) = request.options.get("exit") {
        eprintln!("exiting with status {code} as asked");
        std::process::exit(code.parse().expect("a status"));
    }
    if request.options.contains_key("garbage") {
        print!("this is not JSON");
        return;
    }
    if let Some(msg) = request.options.get("fail") {
        response.errors.push(msg.clone());
    }
    if response.errors.is_empty() {
        let mut lines: Vec<String> = request
            .boundary
            .functions
            .iter()
            .map(|f| format!("{} {} {}", f.lean_name, f.symbol, f.lean_type.replace('\n', " ")))
            .collect();
        lines.sort();
        let mut files = BTreeMap::new();
        files.insert("functions.txt".to_owned(), lines.join("\n") + "\n");
        files.insert("request.json".to_owned(), input);
        response.files = files;
    }
    let out = serde_json::to_string(&response).expect("the response serializes");
    std::io::stdout().write_all(out.as_bytes()).expect("the response is writable");
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().unwrap()
}

/// A `PATH` directory with this program as the plugin `lungo-gen-echo`.
fn plugin_path() -> PathBuf {
    let name = format!("lungo-gen-echo{}", std::env::consts::EXE_SUFFIX);
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-path");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::copy(std::env::current_exe().unwrap(), dir.join(&name)).unwrap();
    dir
}

/// A local distribution: a development build of lungo needs one for any binding. The plugin
/// uses no part of it but its version.
fn distribution() -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("plugin-dist");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("VERSION"), format!("{}\n", env!("CARGO_PKG_VERSION"))).unwrap();
    dir
}

fn generate(out: &Path, opts: &[&str]) -> std::process::Output {
    let path = std::env::join_paths(
        std::iter::once(plugin_path()).chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lungo"));
    cmd.arg("--config")
        .arg(repo().join("compiler-tests/polyglot/lungo.toml"))
        .arg("generate")
        .arg(format!("--echo_out={}", out.display()))
        .arg("--runtime-dir")
        .arg(distribution())
        .env("PATH", path);
    for o in opts {
        cmd.arg(format!("--echo_opt={o}"));
    }
    cmd.output().unwrap()
}

fn a_plugin_receives_the_request_and_its_files_are_published() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("echo-out");
    let run = generate(&out, &[]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let functions = std::fs::read_to_string(out.join("functions.txt")).unwrap();
    assert!(functions.contains("Polyglot.factorial polyglot__call_l_Polyglot_dfactorial Nat → Nat"), "{functions}");
    let request: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["protocol_version"], 1);
    assert_eq!(request["program"]["name"], "polyglot");
    assert!(request["program_files"]["program/lungo.h"].is_string(), "the program's C is in the request");
    assert!(request["runtime"]["distribution"]["local"]["dir"].is_string());
    assert_eq!(request["boundary"]["host_externs"].as_array().unwrap().len(), 2);
    // Type expressions as the protocol reference documents them.
    let mix = request["boundary"]["functions"].as_array().unwrap().iter().find(|f| f["lean_name"] == "Polyglot.mix").unwrap();
    let types: Vec<&serde_json::Value> = mix["params"].as_array().unwrap().iter().map(|p| &p["ty"]).collect();
    assert_eq!(types, ["uint64", "int32", "char", "bool"]);
    assert_eq!(mix["returns"], serde_json::json!({ "value": "string" }));
    // Unchanged inputs: the output is up to date.
    let again = generate(&out, &[]);
    assert!(String::from_utf8_lossy(&again.stdout).starts_with("up to date: echo "));
}

fn plugin_failures_are_reported_and_nothing_is_published() {
    for (opts, needle) in [
        (&["fail=the plugin refuses"][..], "the plugin refuses"),
        (&["garbage=1"][..], "malformed response"),
        (&["exit=3"][..], "exiting with status 3 as asked"),
        (&["colour=blue"][..], "unknown option `colour`"),
    ] {
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("echo-failed");
        let run = generate(&out, opts);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(!run.status.success(), "{opts:?}");
        assert!(stderr.contains("error[LNG0504]") && stderr.contains(needle), "{opts:?}: {stderr}");
        assert!(!out.exists(), "{opts:?}: nothing is published");
    }
}
