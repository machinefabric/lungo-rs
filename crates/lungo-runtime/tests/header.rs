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
