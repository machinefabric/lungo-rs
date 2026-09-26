use lean2rust_bir::DeclarationValue;
use lean2rust_build::Config;
use std::path::PathBuf;

#[test]
fn lake_project_reaches_typed_bir_without_rewriting_source() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let project = root.join("compiler-tests/fixtures/simple");
    let source = project.join("Simple.lean");
    let before = std::fs::read(&source).unwrap();
    let manifest = project.join("lake-manifest.json");
    let manifest_before = std::fs::read(&manifest).unwrap();
    let cache = tempfile::tempdir().unwrap();
    let module = Config::new(&project)
        .root_module("Simple")
        .cache_dir(cache.path())
        .inspect()
        .unwrap();
    assert_eq!(module.module, "Simple");
    assert!(module.declarations.iter().any(|declaration| {
        declaration.name == "providerSend"
            && matches!(declaration.value, DeclarationValue::Extern { .. })
    }));
    assert_eq!(std::fs::read(source).unwrap(), before);
    assert_eq!(std::fs::read(manifest).unwrap(), manifest_before);
}
