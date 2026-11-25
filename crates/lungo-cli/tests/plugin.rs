//! The plugin protocol: `--<name>_out` runs `lungo-gen-<name>` from `PATH` with the request on its
//! standard input, publishes the files of its response, and reports its failures as LNG0504.
//!
//! This test program is also the plugin: run as `lungo-gen-echo`, it reads a `GenerateRequest`
//! and answers a `GenerateResponse`, writing `functions.txt` (each exported function's Lean name,
//! entry point and type) and `request.json` (the request). Its options: `fail` (report an
//! error), `garbage` (write a malformed response), `exit` (exit with this status), `assurance`
//! (write `assurance.json` itself).

use lungo_build::codegen::core::assurance::AssuranceDocument;
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
            "test0268_a_plugin_receives_the_request_and_its_files_are_published",
            test0268_a_plugin_receives_the_request_and_its_files_are_published,
        ),
        (
            "test0269_plugin_failures_are_reported_and_nothing_is_published",
            test0269_plugin_failures_are_reported_and_nothing_is_published,
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
        if !["fail", "garbage", "exit", "assurance"].contains(&key.as_str()) {
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
        if request.options.contains_key("assurance") {
            files.insert("assurance.json".to_owned(), "{}".to_owned());
        }
        response.files = files;
    }
    let out = serde_json::to_string(&response).expect("the response serializes");
    std::io::stdout().write_all(out.as_bytes()).expect("the response is writable");
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).ancestors().nth(2).unwrap().to_path_buf()
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

/// TEST0268: a plugin receives the request and its files are published (run by this file's own harness, `main`)
fn test0268_a_plugin_receives_the_request_and_its_files_are_published() {
    let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("echo-out");
    let run = generate(&out, &[]);
    assert!(run.status.success(), "{}", String::from_utf8_lossy(&run.stderr));
    let functions = std::fs::read_to_string(out.join("functions.txt")).unwrap();
    assert!(functions.contains("Polyglot.factorial polyglot__call_l_Polyglot_dfactorial Nat → Nat"), "{functions}");
    let request: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(out.join("request.json")).unwrap()).unwrap();
    assert_eq!(request["protocol_version"], 4);
    // Docstrings, as the protocol reference documents them: of a type, its constructors and its
    // structure fields, a function and an operation; `null` where the declaration has none.
    let types = request["boundary"]["types"].as_array().unwrap();
    let ty = |name: &str| types.iter().find(|t| t["lean_name"] == name).unwrap_or_else(|| panic!("no {name}"));
    let doc = |v: &serde_json::Value| v.as_str().map(str::trim).map(str::to_owned);
    assert!(doc(&ty("Polyglot.Lookup")["doc"]).unwrap().starts_with("What looking a slot up answers."));
    let point = ty("Polyglot.Point");
    assert!(point["doc"].is_null(), "{point}");
    let fields = point["ctors"][0]["fields"].as_array().unwrap();
    assert_eq!(fields.len(), 4, "{point}");
    assert!(doc(&fields[0]).unwrap().starts_with("The distance from the left edge,"), "{point}");
    assert!(fields[1].is_null() && fields[3].is_null(), "{point}");
    assert_eq!(doc(&fields[2]).as_deref(), Some("What the point is called."));
    let shape = ty("Polyglot.Shape");
    assert_eq!(doc(&shape["ctors"][0]["doc"]).as_deref(), Some("A disc around `center`."));
    assert!(shape["ctors"][1]["doc"].is_null() && shape["ctors"][0]["fields"][0].is_null(), "{shape}");
    let function = |name: &str| {
        request["boundary"]["functions"].as_array().unwrap().iter().find(|f| f["lean_name"] == name).unwrap()
    };
    assert_eq!(doc(&function("Polyglot.factorial")["doc"]).as_deref(), Some("Natural numbers of any size."));
    assert!(function("Polyglot.area")["doc"].is_null());
    let scaler = request["boundary"]["facilities"].as_array().unwrap().iter().find(|c| c["id"] == "polyglot.scaler");
    assert_eq!(doc(&scaler.unwrap()["operations"][0]["doc"]).as_deref(), Some("Scales by the host's factor (pure)."));
    assert_eq!(request["program"]["name"], "polyglot");
    assert_eq!(request["extern_types"], serde_json::json!({}), "no language table names extern types");
    // Every type carries whether it is opaque and its layout fingerprint.
    for t in request["boundary"]["types"].as_array().unwrap() {
        assert!(t["opaque"].is_boolean(), "{t}");
        let fp = t["fingerprint"].as_str().unwrap();
        assert!(fp.len() == 64 && fp.bytes().all(|b| b.is_ascii_hexdigit()), "{t}");
    }
    assert!(request["program_files"]["program/lungo.h"].is_string(), "the program's C is in the request");
    assert!(request["runtime"]["distribution"]["local"]["dir"].is_string());
    // The host's facilities, each with its operations, and the async one.
    let facilities: Vec<(&str, usize)> = request["boundary"]["facilities"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| (c["id"].as_str().unwrap(), c["operations"].as_array().unwrap().len()))
        .collect();
    assert_eq!(facilities, [("polyglot.journal", 1), ("polyglot.scaler", 1)]);
    let asyncs = request["boundary"]["async_facilities"].as_array().unwrap();
    assert_eq!((asyncs.len(), asyncs[0]["id"].as_str().unwrap()), (1, "polyglot.fetch"));
    assert_eq!(asyncs[0]["operations"].as_array().unwrap().len(), 3);
    // The assurance document is in the request, and lungo writes it into the output itself, as
    // the document serializes.
    let document: AssuranceDocument = serde_json::from_value(request["assurance"].clone()).unwrap();
    assert_eq!(document.program, "polyglot");
    assert!(document.claims.iter().any(|c| c.name == "Polyglot.factorial_pos"), "the claims are in the request");
    assert_eq!(std::fs::read_to_string(out.join("assurance.json")).unwrap(), document.to_json());
    // Type expressions as the protocol reference documents them.
    let mix =
        request["boundary"]["functions"].as_array().unwrap().iter().find(|f| f["lean_name"] == "Polyglot.mix").unwrap();
    let types: Vec<&serde_json::Value> = mix["params"].as_array().unwrap().iter().map(|p| &p["ty"]).collect();
    assert_eq!(types, ["uint64", "int32", "char", "bool"]);
    assert_eq!(mix["returns"], serde_json::json!({ "value": "string" }));
    // Unchanged inputs: the output is up to date.
    let again = generate(&out, &[]);
    assert!(String::from_utf8_lossy(&again.stdout).starts_with("up to date: echo "));
}

/// TEST0269: plugin failures are reported and nothing is published (run by this file's own harness, `main`)
fn test0269_plugin_failures_are_reported_and_nothing_is_published() {
    for (opts, needle) in [
        (&["fail=the plugin refuses"][..], "the plugin refuses"),
        (&["garbage=1"][..], "malformed response"),
        (&["exit=3"][..], "exiting with status 3 as asked"),
        (&["colour=blue"][..], "unknown option `colour`"),
        (&["assurance=1"][..], "assurance.json"),
    ] {
        let out = Path::new(env!("CARGO_TARGET_TMPDIR")).join("echo-failed");
        let run = generate(&out, opts);
        let stderr = String::from_utf8_lossy(&run.stderr);
        assert!(!run.status.success(), "{opts:?}");
        assert!(stderr.contains("error[LNG0504]") && stderr.contains(needle), "{opts:?}: {stderr}");
        assert!(!out.exists(), "{opts:?}: nothing is published");
    }
}
