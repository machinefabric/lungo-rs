//! Two Lean projects whose generated modules share types, with application-shaped facades.

/// The `geometry` Lake package, with serde support added to its types.
pub mod geometry {
    lungo::include_lean!("geometry");

    /// `Secret` is generated without `Debug` so that its code never appears in logs.
    impl std::fmt::Debug for Secret {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("Secret(<redacted>)")
        }
    }
}

/// The `drawing` Lake package, which requires `geometry` and uses its Rust types.
pub mod drawing {
    lungo::include_lean!("drawing");
}
