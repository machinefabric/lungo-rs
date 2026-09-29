//! Every error code is documented in `docs/reference/errors.md` and its Italian twin
//! `errors.it.md` — the pages the documentation site publishes at each release — and neither
//! documents a code that does not exist.

use lungo_driver::ErrorCode;
use std::collections::BTreeSet;

fn assert_documents_every_code(page: &str) {
    let path = format!("{}/../../docs/reference/{page}", env!("CARGO_MANIFEST_DIR"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    let documented: BTreeSet<&str> = text.lines().filter_map(|l| l.strip_prefix("### ")).map(str::trim).collect();
    let defined: BTreeSet<&str> = ErrorCode::ALL.iter().map(|c| c.as_str()).collect();
    let undocumented: Vec<_> = defined.difference(&documented).collect();
    let unknown: Vec<_> = documented.difference(&defined).collect();
    assert!(undocumented.is_empty(), "codes missing from {path}: {undocumented:?}");
    assert!(unknown.is_empty(), "{path} documents codes that do not exist: {unknown:?}");
}

#[test]
fn error_reference_matches_the_codes() {
    assert_documents_every_code("errors.md");
}

#[test]
fn italian_error_reference_matches_the_codes() {
    assert_documents_every_code("errors.it.md");
}
