//! `include/lungo.h` declares every runtime primitive the library exports, exactly as the
//! registry defines it. Set `LUNGO_BLESS=1` to regenerate the section after changing the
//! registry.

use std::path::Path;

/// TEST0242: header declares every primitive of the registry
#[test]
fn test0242_header_declares_every_primitive_of_the_registry() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("include/lungo.h");
    let current = std::fs::read_to_string(&path).unwrap();
    let expected =
        lungo_runtime::header::with_primitives(&current, &lungo_runtime::header::primitive_declarations()).unwrap();
    if std::env::var_os("LUNGO_BLESS").is_some() {
        std::fs::write(&path, &expected).unwrap();
        return;
    }
    assert!(
        current == expected,
        "include/lungo.h is out of date with the runtime's primitives; run `LUNGO_BLESS=1 cargo test -p lungo-runtime --test header`"
    );
}

/// The `lungo_*` functions the C ABI modules (`src/capi`) export; `capi/wasm.rs`, the
/// WebAssembly module's interface to its JavaScript host, is not part of it.
fn capi_exports() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/capi");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.file_name().is_some_and(|n| n == "wasm.rs") {
            continue;
        }
        let source = std::fs::read_to_string(path).unwrap();
        let mut exported = false;
        for line in source.lines().map(str::trim) {
            if line == "#[unsafe(no_mangle)]" {
                exported = true;
            } else if exported && !line.starts_with("#[") && !line.starts_with("///") {
                exported = false;
                let name = line.split("fn ").nth(1).and_then(|rest| rest.split(['(', '<']).next());
                if let Some(name) = name.filter(|_| line.contains("extern \"C\" fn ")) {
                    out.push(name.to_owned());
                }
            }
        }
    }
    out.sort();
    out
}

/// TEST0277: header declares every function of the C ABI
#[test]
fn test0277_header_declares_every_function_of_the_c_abi() {
    let header = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("include/lungo.h")).unwrap();
    let declared = |name: &str| header.contains(&format!(" {name}(")) || header.contains(&format!("*{name}("));
    let exports = capi_exports();
    for f in ["lungo_async_resume", "lungo_async_cancel", "lungo_async_outstanding", "lungo_resume", "lungo_abi_v2"] {
        assert!(exports.iter().any(|e| e == f), "{f} is not found among the exports: the scan is broken");
    }
    let missing: Vec<&String> = exports.iter().filter(|e| e.starts_with("lungo_") && !declared(e)).collect();
    assert!(missing.is_empty(), "include/lungo.h does not declare {missing:?}");
    // What a WebAssembly module exports for the TypeScript binding, but for its interface to the
    // host, is in the C ABI.
    let absent: Vec<&&str> = lungo_runtime::header::WASM_EXPORTS
        .iter()
        .filter(|e| !e.starts_with("lungo_wasm_") && !declared(e))
        .collect();
    assert!(absent.is_empty(), "the WebAssembly exports {absent:?} are not declared in include/lungo.h");
}
