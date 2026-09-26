/// A one-based line and column in a Lean source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourcePosition {
    pub line: u32,
    pub column: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceRange {
    pub start: SourcePosition,
    pub end: SourcePosition,
}

/// The trust assumptions an exported declaration's compiled code depends on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExportTrust {
    /// Axioms the definition depends on, excluding `sorryAx`.
    pub axioms: &'static [&'static str],
    pub depends_on_sorry: bool,
    /// `unsafe` constants in the executable closure: the declaration itself, and those outside
    /// the Lean toolchain.
    pub unsafe_dependencies: &'static [&'static str],
    /// `partial` constants in the executable closure, with the same scope.
    pub partial_dependencies: &'static [&'static str],
    /// External symbols the compiled code calls.
    pub extern_dependencies: &'static [&'static str],
}

/// Static metadata about a Lean declaration with a generated Rust facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclarationInfo {
    /// The fully qualified Lean name: the canonical identity of the declaration.
    pub lean_name: &'static str,
    pub module: &'static str,
    /// The Lean source file, relative to the Cargo package (or `<package>/…` for dependencies).
    pub source_file: Option<&'static str>,
    pub range: Option<SourceRange>,
    /// The declaration's Lean type.
    pub lean_type: &'static str,
    /// The signature of the compiled declaration in runtime representation types.
    pub compiled_signature: &'static str,
    /// The path of the generated Rust item, relative to the generated module.
    pub rust_path: &'static str,
    pub trust: ExportTrust,
}
