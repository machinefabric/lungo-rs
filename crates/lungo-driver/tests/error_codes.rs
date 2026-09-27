//! Every error code is documented in `docs/src/content/reference/errors.md`, and the reference documents
//! no code that does not exist.

use lungo_build::ErrorCode;
use std::collections::BTreeSet;

#[test]
fn error_reference_matches_the_codes() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../docs/src/content/reference/errors.md");
    let text = std::fs::read_to_string(path).unwrap();
    let documented: BTreeSet<&str> = text.lines().filter_map(|l| l.strip_prefix("### ")).map(str::trim).collect();
    let defined: BTreeSet<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    let undocumented: Vec<_> = defined.difference(&documented).collect();
    let unknown: Vec<_> = documented.difference(&defined).collect();
    assert!(undocumented.is_empty(), "codes missing from {path}: {undocumented:?}");
    assert!(unknown.is_empty(), "{path} documents codes that do not exist: {unknown:?}");
}
