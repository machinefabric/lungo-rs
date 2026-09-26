//! The lean2rust Rust backend: generates Rust from Bridge IR.
//!
//! The output has two layers. The compiler layer (`__l2r`) mechanically reproduces Lean's
//! compiled program on top of the `lean2rust` runtime, one file per Lean module. The public
//! facade exposes exported declarations as ordinary Rust functions over idiomatic Rust types.
//! Alongside the Rust sources, machine-readable metadata records name mappings, extern
//! resolutions, and source provenance.

mod compiler;
mod externs;
mod facade;
mod names;
mod oracle;
mod rust;

pub use externs::{Resolution, resolution_key};
pub use names::{mangle, module_file_stem};
pub use oracle::C_SHIM as ORACLE_C_SHIM;

use compiler::Emitter;
use externs::ExternPlan;
use facade::{Facade, FnSpec, ModuleTree, Naming};
use lean2rust_bir::{Body, Declaration, Initializer, IrType, ValidationError};
use lean2rust_protocol::{
    DeclSource, EntryPoint, ExternRequirement, PackageOrigin, SourceLocation, Success, WorkerToolchain,
};
use rust::Writer;
use serde::Serialize;
use std::collections::{BTreeMap, HashMap};
use std::fmt;

/// Version of the generated-code format; part of the build fingerprint.
pub const GENERATOR_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug)]
pub enum CodegenError {
    /// The program violates BIR invariants.
    Validation(Vec<ValidationError>),
    /// The compiler output contains a construct this backend does not implement.
    Adapter { declaration: String, message: String },
    /// An extern cannot be resolved or implemented.
    Extern(String),
    /// A violated internal invariant of the generator.
    Internal(String),
}

impl CodegenError {
    pub fn adapter(declaration: &str, message: impl Into<String>) -> Self {
        CodegenError::Adapter { declaration: declaration.to_owned(), message: message.into() }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        CodegenError::Internal(message.into())
    }
}

impl fmt::Display for CodegenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodegenError::Validation(errors) => {
                writeln!(f, "the Bridge IR produced by the Lean worker is invalid:")?;
                for e in errors.iter().take(50) {
                    writeln!(f, "  {e}")?;
                }
                if errors.len() > 50 {
                    writeln!(f, "  ... and {} more", errors.len() - 50)?;
                }
                Ok(())
            }
            CodegenError::Adapter { declaration, message } => write!(
                f,
                "lean2rust backend does not implement this compiler output: {message}\n\nLean declaration: {declaration}"
            ),
            CodegenError::Extern(message) => f.write_str(message),
            CodegenError::Internal(message) => write!(f, "internal lean2rust code generator error: {message}"),
        }
    }
}

impl std::error::Error for CodegenError {}

/// A generated public item and the Lean declaration it represents.
#[derive(Debug, Clone, Serialize)]
pub struct NameRecord {
    pub lean_name: String,
    pub kind: String,
    pub rust_path: String,
    /// Whether sanitization changed the identifier.
    pub renamed: bool,
}

/// Human-readable locations of Lean declarations, for comments and metadata.
pub struct SourceIndex {
    entries: HashMap<String, (String, Option<(u32, u32)>)>,
}

impl SourceIndex {
    fn new(entries: &[lean2rust_protocol::SourceEntry], local_prefix: &str) -> Self {
        let entries = entries
            .iter()
            .map(|e| {
                (
                    e.name.clone(),
                    (
                        display_path(&e.source.location, local_prefix),
                        e.source.range.map(|r| (r.start.line, r.start.column)),
                    ),
                )
            })
            .collect();
        SourceIndex { entries }
    }

    /// `path:line:column` for `lean_name`.
    pub fn describe(&self, lean_name: &str) -> Option<String> {
        self.entries.get(lean_name).map(|(p, r)| match r {
            Some((l, c)) => format!("{p}:{l}:{c}"),
            None => p.clone(),
        })
    }
}

/// A source location without machine-specific absolute paths: local files relative to the Cargo
/// package, dependency and toolchain files under a symbolic root.
pub fn display_path(loc: &SourceLocation, local_prefix: &str) -> String {
    let join = |a: &str, b: &str| {
        if a.is_empty() { b.to_owned() } else { format!("{}/{b}", a.trim_end_matches('/')) }
    };
    match &loc.origin {
        PackageOrigin::Root => join(local_prefix, &loc.path),
        PackageOrigin::Path { dir } => join(&join(local_prefix, dir), &loc.path),
        PackageOrigin::Git { .. } => format!("<package:{}>/{}", loc.package, loc.path),
        PackageOrigin::Toolchain => format!("<lean>/{}", loc.path),
    }
}

fn describe_source(src: &Option<DeclSource>, local_prefix: &str) -> Option<String> {
    src.as_ref().map(|s| {
        let p = display_path(&s.location, local_prefix);
        match s.range {
            Some(r) => format!("{p}:{}:{}", r.start.line, r.start.column),
            None => p,
        }
    })
}

/// What executes the Lean program behind the facade.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layer {
    /// Generated Rust on the lean2rust runtime.
    PureRust,
    /// Lean's natively compiled code and C runtime, reached through FFI.
    Oracle,
}

pub struct GenInput<'a> {
    pub layer: Layer,
    pub success: &'a Success,
    pub toolchain: &'a WorkerToolchain,
    /// The Lean namespace whose contents appear at the root of the generated module.
    pub facade_namespace: &'a str,
    /// Stem of the aggregate include file (`<aggregate>.rs`).
    pub aggregate: &'a str,
    /// Application-provided implementations of extern symbols: symbol → Rust path.
    pub rust_externs: &'a BTreeMap<String, String>,
    /// Path of the Lean project relative to the Cargo package, used in source references.
    pub local_prefix: &'a str,
    /// Source text of local modules to embed, keyed by module name, when requested.
    pub embedded_sources: Option<&'a BTreeMap<String, String>>,
}

/// The generated files, keyed by path relative to the output directory.
pub struct Generated {
    pub files: BTreeMap<String, String>,
}

/// Generates Rust for the worker's response.
pub fn generate(input: &GenInput) -> Result<Generated, Vec<CodegenError>> {
    let program = &input.success.bir;
    lean2rust_bir::validate(program).map_err(|e| vec![CodegenError::Validation(e)])?;
    let sources = SourceIndex::new(&input.success.source_metadata, input.local_prefix);
    let requirements: HashMap<&str, &ExternRequirement> =
        input.success.extern_requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
    let externs =
        ExternPlan::resolve(&program.declarations, &input.success.extern_requirements, input.rust_externs, &|r| {
            describe_source(&r.source, input.local_prefix)
        })?;
    let mut init_values = HashMap::new();
    for m in &program.modules {
        for init in &m.initializers {
            if let Initializer::Value { decl, init_fn } = init {
                init_values.insert(decl.as_str(), init_fn.as_str());
            }
        }
    }
    let emitter = Emitter {
        decls: program.declarations.iter().map(|d| (d.name.as_str(), d)).collect(),
        externs: &externs,
        sources: &sources,
        init_values,
    };
    let mut errors = Vec::new();
    let mut files = BTreeMap::new();

    // Compiler layer: one file per Lean module.
    let mut by_module: BTreeMap<&str, Vec<&Declaration>> = BTreeMap::new();
    for d in &program.declarations {
        by_module.entry(d.module.as_str()).or_default().push(d);
    }
    let mut module_files = Vec::new();
    // Distinct stems can still coincide on case-insensitive file systems.
    let mut folded: HashMap<String, &str> = HashMap::new();
    for module in by_module.keys() {
        if let Some(other) = folded.insert(module_file_stem(module).to_lowercase(), module) {
            return Err(vec![CodegenError::internal(format!(
                "Lean modules {other} and {module} differ only in letter case; their generated files would collide"
            ))]);
        }
    }
    for (module, decls) in by_module.iter().filter(|_| input.layer == Layer::PureRust) {
        let mut w = Writer::new();
        w.line(format!("// Compiled Lean module {module}. Generated by lean2rust; do not edit."));
        w.line("");
        for d in decls {
            if let Err(e) = emitter.declaration(&mut w, d) {
                errors.push(e);
            }
        }
        let path = format!("modules/{}.rs", module_file_stem(module));
        files.insert(path.clone(), w.finish());
        module_files.push(path);
    }

    // Public facade: the types reachable from exports and application-provided externs.
    let interface = &input.success.interface;
    let user_externs: Vec<&ExternRequirement> = externs
        .resolutions
        .iter()
        .filter(|(_, r)| matches!(r, Resolution::User { .. }))
        .filter_map(|(d, _)| requirements.get(d.as_str()).copied())
        .collect();
    let reachable = facade::reachable_types(&interface.types, &interface.exports, &user_externs);
    let types: Vec<lean2rust_protocol::TypeDecl> =
        interface.types.iter().filter(|t| reachable.contains(t.name.as_str())).cloned().collect();
    let mut naming = Naming::new(input.facade_namespace);
    let backend_module = match input.layer {
        Layer::PureRust => None,
        Layer::Oracle => Some("__oracle".to_owned()),
    };
    let mut facade = match Facade::new(&types, &mut naming, &interface.exports, backend_module) {
        Ok(f) => f,
        Err(e) => return Err(vec![e]),
    };
    let mut tree = ModuleTree::default();
    if let Err(e) = facade.emit_types(&mut tree, &types) {
        errors.push(e);
    }
    let decl_of = |n: &str| program.declaration(n);
    for export in &interface.exports {
        let Some(decl) = decl_of(&export.name) else {
            errors.push(CodegenError::internal(format!("export {} has no compiled declaration", export.name)));
            continue;
        };
        let spec = FnSpec {
            lean_name: &decl.name,
            ir_params: &decl.params,
            ir_result: decl.result,
            type_params: &export.type_params,
            params: &export.params,
            result: &export.result,
        };
        let doc = facade::describe_export(export, describe_source(&export.source, input.local_prefix));
        if let Err(e) = facade.emit_function(&mut tree, export, &spec, &doc) {
            errors.push(e);
        }
    }
    let mut adapters = Writer::new();
    for (decl_name, resolution) in &externs.resolutions {
        if let Resolution::User { rust_path, key } = resolution {
            let decl = decl_of(decl_name).expect("resolved externs are declarations");
            let export_symbol = match (input.layer, &decl.body) {
                (Layer::PureRust, _) => None,
                (Layer::Oracle, Body::Extern { selected: lean2rust_bir::ExternEntry::Standard { .. }, .. }) => {
                    Some(key.as_str())
                }
                (Layer::Oracle, _) => {
                    errors.push(CodegenError::Extern(format!(
                        "LeanOracle mode can only provide application externs declared with `@[extern \"symbol\"]`; {decl_name} uses another extern form"
                    )));
                    continue;
                }
            };
            match requirements.get(decl_name.as_str()) {
                Some(req) => {
                    if let Err(e) = facade.emit_extern_adapter(&mut adapters, decl, req, rust_path, export_symbol) {
                        errors.push(e);
                    }
                }
                None => errors.push(CodegenError::internal(format!("no extern requirement for {decl_name}"))),
            }
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }
    let adapters = adapters.finish();

    // The aggregate include target.
    let mut w = Writer::new();
    w.line(format!(
        "// Generated by lean2rust {GENERATOR_VERSION} from Lean {} (Bridge IR {}). Do not edit.",
        input.toolchain.lean_version, program.bir_version
    ));
    w.line("");
    w.line("#[doc(hidden)]");
    w.line("#[allow(non_snake_case, non_upper_case_globals, unused_mut, unused_variables, unused_assignments, unused_labels, unused_unsafe, unused_imports, unreachable_code, unreachable_patterns, dead_code, unsafe_op_in_unsafe_fn, clippy::all)]");
    w.open("pub mod __l2r {");
    w.line("use ::lean2rust::__runtime as rt;");
    w.line("use rt::Obj;");
    w.line(format!("const _: () = rt::assert_abi::<{}>();", lean2rust_runtime::ABI_VERSION));
    w.line("");
    match input.layer {
        Layer::PureRust => {
            for path in &module_files {
                w.line(format!("include!({});", rust::string(path)));
            }
            w.line("");
            for line in adapters.lines() {
                w.line(line);
            }
            if let Err(e) = emit_runtime_exports(&mut w, input) {
                return Err(vec![e]);
            }
            emit_initialize(&mut w, input, &emitter);
        }
        Layer::Oracle => {
            if let Err(e) = oracle::emit_native_layer(&mut w, input) {
                return Err(vec![e]);
            }
        }
    }
    w.close("}");
    if input.layer == Layer::Oracle {
        w.line("");
        w.line("#[doc(hidden)]");
        w.line("#[allow(non_snake_case, dead_code, clippy::all)]");
        w.open("pub mod __oracle {");
        for line in adapters.lines() {
            w.line(line);
        }
        if let Err(e) = oracle::emit_backend(&mut w, input) {
            return Err(vec![e]);
        }
        w.close("}");
    }
    w.line("");
    tree.render(&mut w);
    w.line("");
    w.line("#[doc(hidden)]");
    facade.emit_markers(&mut w);
    w.line("");
    emit_meta(&mut w, input, &naming);
    if let Some(entry) = &input.success.entry_point {
        emit_entry_point(&mut w, entry, program, input.layer);
    }
    files.insert(format!("{}.rs", input.aggregate), w.finish());

    // Machine-readable metadata.
    // Items first (in placement order), then the constructors and fields of emitted types.
    let names: Vec<&NameRecord> = naming.records.iter().chain(&facade.members).collect();
    files.insert("names.json".into(), to_json(&names));
    files.insert("externs.json".into(), to_json(&extern_report(&externs, input)));
    files.insert("sources.json".into(), to_json(&sources_report(input)));
    files.insert("manifest.json".into(), to_json(&manifest_report(input, &naming)));
    Ok(Generated { files })
}

fn to_json<T: Serialize>(v: &T) -> String {
    let mut s = serde_json::to_string_pretty(v).expect("metadata serializes");
    s.push('\n');
    s
}

fn emit_initialize(w: &mut Writer, input: &GenInput, emitter: &Emitter) {
    let program = &input.success.bir;
    w.line("/// Initializes the Lean program: runs every module initializer once, in import order.");
    w.open("pub fn __initialize() {");
    w.line("static INIT: ::std::sync::Once = ::std::sync::Once::new();");
    w.open("INIT.call_once(|| unsafe {");
    for export in &input.success.runtime_exports {
        w.line(format!(
            "rt::exports::register({}, e_{} as *const ());",
            rust::string(&export.symbol),
            mangle(&export.declaration)
        ));
    }
    for m in &program.modules {
        if m.initializers.is_empty() {
            continue;
        }
        w.line(format!("// {}", m.name));
        for init in &m.initializers {
            match init {
                Initializer::Io(decl) => {
                    w.line(format!("rt::lean_dec(rt::check_initializer({}, {}()));", rust::string(decl), mangle(decl)));
                }
                Initializer::Value { decl, init_fn } => {
                    let d = emitter.decls[decl.as_str()];
                    let v = format!("rt::check_initializer({}, {}())", rust::string(decl), mangle(init_fn));
                    let set = match d.result {
                        IrType::Uint8 | IrType::Uint16 => {
                            format!("I_{}.set(rt::lean_unbox(v) as {})", mangle(decl), compiler::rust_type(d.result))
                        }
                        IrType::Uint32 => format!("I_{}.set(rt::lean_unbox_uint32(v))", mangle(decl)),
                        IrType::Uint64 => format!("I_{}.set(rt::lean_unbox_uint64(v))", mangle(decl)),
                        IrType::Usize => format!("I_{}.set(rt::lean_unbox_usize(v))", mangle(decl)),
                        IrType::Float => format!("I_{}.set(rt::lean_unbox_float(v))", mangle(decl)),
                        IrType::Float32 => format!("I_{}.set(rt::lean_unbox_float32(v))", mangle(decl)),
                        _ => format!("I_{}.set_obj(v)", mangle(decl)),
                    };
                    let release = if d.result.is_scalar() { " rt::lean_dec(v);" } else { "" };
                    w.line(format!("{{ let v = {v}; {set};{release} }}"));
                }
            }
        }
    }
    w.line("rt::io::mark_end_initialization();");
    w.close("});");
    w.close("}");
}

/// Emits wrappers giving the Lean definitions the runtime calls the C signature the runtime
/// uses, reconciling ownership conventions.
fn emit_runtime_exports(w: &mut Writer, input: &GenInput) -> Result<(), CodegenError> {
    use lean2rust_runtime::registry::Ty;
    for export in &input.success.runtime_exports {
        let spec = lean2rust_runtime::exports::REQUIRED
            .iter()
            .find(|r| r.symbol == export.symbol)
            .ok_or_else(|| CodegenError::internal(format!("the runtime does not call export {}", export.symbol)))?;
        let decl = input
            .success
            .bir
            .declaration(&export.declaration)
            .ok_or_else(|| CodegenError::internal(format!("{} is not part of the program", export.declaration)))?;
        let params: Vec<&lean2rust_bir::Param> = decl.params.iter().filter(|p| p.ty != IrType::Void).collect();
        let matches = params.len() == spec.params.len()
            && params.iter().zip(spec.params).all(|(p, t)| match t {
                Ty::obj | Ty::b_obj => p.ty.is_object(),
                Ty::u8 => p.ty == IrType::Uint8,
                Ty::u16 => p.ty == IrType::Uint16,
                Ty::u32 => p.ty == IrType::Uint32,
                Ty::u64 => p.ty == IrType::Uint64,
                Ty::usize => p.ty == IrType::Usize,
                Ty::f64 => p.ty == IrType::Float,
                Ty::f32 => p.ty == IrType::Float32,
            })
            && decl.result.is_object()
            && matches!(spec.result, Ty::obj);
        if !matches {
            return Err(CodegenError::adapter(
                &decl.name,
                format!(
                    "the Lean definition exported as `{}` does not have the signature the runtime calls",
                    spec.symbol
                ),
            ));
        }
        let mut sig = Vec::new();
        let mut pre = Vec::new();
        let mut post = Vec::new();
        let mut args = Vec::new();
        for (i, (p, t)) in params.iter().zip(spec.params).enumerate() {
            sig.push(format!("a_{i}: {}", compiler::rust_type(p.ty)));
            if p.ty.is_object() {
                match (t, p.borrow) {
                    (Ty::obj, true) => post.push(format!("rt::lean_dec(a_{i});")),
                    (Ty::b_obj, false) => pre.push(format!("rt::lean_inc(a_{i});")),
                    _ => {}
                }
            }
            args.push(format!("a_{i}"));
        }
        w.line(format!("// The runtime calls {} as `{}`.", decl.name, spec.symbol));
        w.open(format!("unsafe extern \"C\" fn e_{}({}) -> Obj {{", mangle(&decl.name), sig.join(", ")));
        for l in pre {
            w.line(l);
        }
        w.line(format!("let r = {}({});", mangle(&decl.name), args.join(", ")));
        for l in post {
            w.line(l);
        }
        w.line("r");
        w.close("}");
        w.line("");
    }
    Ok(())
}

fn emit_entry_point(w: &mut Writer, entry: &EntryPoint, program: &lean2rust_bir::Program, layer: Layer) {
    let main = program.declaration(&entry.declaration);
    let Some(main) = main else { return };
    let with_args = main.params.iter().any(|p| p.ty != IrType::Void);
    let kind = if with_args { "WithArgs" } else { "NoArgs" };
    w.line("");
    w.line("/// Runs the Lean program's `main` with the process arguments and returns its exit code.");
    w.open("pub fn __lean_main() -> i32 {");
    w.line("__lean_main_with(::std::env::args().skip(1).collect())");
    w.close("}");
    w.line("");
    w.line("/// Runs the Lean program's `main` with `args` and returns its exit code.");
    w.open("pub fn __lean_main_with(args: ::std::vec::Vec<::std::string::String>) -> i32 {");
    match layer {
        Layer::PureRust => w.line(format!(
            "::lean2rust::__runtime::run_main(__l2r::__initialize, ::lean2rust::__runtime::MainFn::{kind}(__l2r::{}), {}, args)",
            mangle(&main.name),
            entry.returns_exit_code
        )),
        Layer::Oracle => w.line("__oracle::run_main(args)"),
    }
    w.close("}");
}

fn emit_meta(w: &mut Writer, input: &GenInput, naming: &Naming) {
    let interface = &input.success.interface;
    let paths: HashMap<&str, &str> =
        naming.records.iter().map(|r| (r.lean_name.as_str(), r.rust_path.as_str())).collect();
    let program = &input.success.bir;
    w.line("/// Static metadata about the generated declarations.");
    w.open("pub mod __meta {");
    w.line("/// The Lean release the code was compiled with.");
    w.line(format!("pub const LEAN_VERSION: &str = {};", rust::string(&input.toolchain.lean_version)));
    w.line(format!("pub const LEAN_GITHASH: &str = {};", rust::string(&input.toolchain.lean_githash)));
    w.line(format!("pub const BIR_VERSION: u32 = {};", program.bir_version));
    w.line(format!("pub const GENERATOR_VERSION: &str = {};", rust::string(GENERATOR_VERSION)));
    w.line("");
    let list = |xs: &[String]| format!("&[{}]", xs.iter().map(|x| rust::string(x)).collect::<Vec<_>>().join(", "));
    let mut exports: Vec<_> = interface.exports.iter().collect();
    exports.sort_by(|a, b| a.name.cmp(&b.name));
    w.open("static DECLARATIONS: &[::lean2rust::DeclarationInfo] = &[");
    for e in &exports {
        let decl = program.declaration(&e.name).expect("exports are compiled");
        let params: Vec<String> =
            decl.params.iter().map(|p| format!("{}{}", if p.borrow { "@& " } else { "" }, p.ty.name())).collect();
        let compiled = format!("({}) -> {}", params.join(", "), decl.result.name());
        let (file, range) = match &e.source {
            Some(s) => (
                format!("Some({})", rust::string(&display_path(&s.location, input.local_prefix))),
                match s.range {
                    Some(r) => format!(
                        "Some(::lean2rust::SourceRange {{ start: ::lean2rust::SourcePosition {{ line: {}, column: {} }}, end: ::lean2rust::SourcePosition {{ line: {}, column: {} }} }})",
                        r.start.line, r.start.column, r.end.line, r.end.column
                    ),
                    None => "None".into(),
                },
            ),
            None => ("None".into(), "None".into()),
        };
        w.open("::lean2rust::DeclarationInfo {");
        w.line(format!("lean_name: {},", rust::string(&e.name)));
        w.line(format!("module: {},", rust::string(&e.module)));
        w.line(format!("source_file: {file},"));
        w.line(format!("range: {range},"));
        w.line(format!("lean_type: {},", rust::string(&e.lean_type)));
        w.line(format!("compiled_signature: {},", rust::string(&compiled)));
        w.line(format!("rust_path: {},", rust::string(paths.get(e.name.as_str()).copied().unwrap_or(""))));
        w.open("trust: ::lean2rust::ExportTrust {");
        w.line(format!("axioms: {},", list(&e.trust.axioms)));
        w.line(format!("depends_on_sorry: {},", e.trust.depends_on_sorry));
        w.line(format!("unsafe_dependencies: {},", list(&e.trust.unsafe_dependencies)));
        w.line(format!("partial_dependencies: {},", list(&e.trust.partial_dependencies)));
        w.line(format!("extern_dependencies: {},", list(&e.trust.extern_dependencies)));
        w.close("},");
        w.close("},");
    }
    w.close("];");
    w.line("");
    w.line("/// Metadata of the exported Lean declaration with fully qualified name `lean_name`.");
    w.open("pub fn declaration(lean_name: &str) -> ::core::option::Option<&'static ::lean2rust::DeclarationInfo> {");
    w.line("DECLARATIONS.binary_search_by(|d| d.lean_name.cmp(lean_name)).ok().map(|i| &DECLARATIONS[i])");
    w.close("}");
    w.line("");
    w.line("/// Metadata of every exported declaration, sorted by Lean name.");
    w.open("pub fn declarations() -> &'static [::lean2rust::DeclarationInfo] {");
    w.line("DECLARATIONS");
    w.close("}");
    if let Some(sources) = input.embedded_sources {
        w.line("");
        w.open("static SOURCES: &[(&str, &str)] = &[");
        for (module, text) in sources {
            w.line(format!("({}, {}),", rust::string(module), rust::string(text)));
        }
        w.close("];");
        w.line("");
        w.line("/// The embedded Lean source text of `module`.");
        w.open("pub fn module_source(module: &str) -> ::core::option::Option<&'static str> {");
        w.line("SOURCES.binary_search_by(|(m, _)| (*m).cmp(module)).ok().map(|i| SOURCES[i].1)");
        w.close("}");
        w.line("");
        w.line("/// The embedded Lean source text of the module defining the exported declaration `lean_name`.");
        w.open("pub fn source(lean_name: &str) -> ::core::option::Option<&'static str> {");
        w.line("declaration(lean_name).and_then(|d| module_source(d.module))");
        w.close("}");
    }
    w.close("}");
}

#[derive(Serialize)]
struct ExternReportEntry {
    declaration: String,
    key: String,
    resolution: String,
    implementation: String,
    lean_type: Option<String>,
    source: Option<String>,
}

fn extern_report(plan: &ExternPlan, input: &GenInput) -> Vec<ExternReportEntry> {
    let reqs: HashMap<&str, &ExternRequirement> =
        input.success.extern_requirements.iter().map(|r| (r.declaration.as_str(), r)).collect();
    let program = &input.success.bir;
    plan.resolutions
        .iter()
        .map(|(decl, res)| {
            let d = program.declaration(decl).expect("resolved extern");
            let key = resolution_key(d).expect("resolved extern has a key");
            let (resolution, implementation) = match res {
                Resolution::Lean { implementation } => ("lean_export", implementation.clone()),
                Resolution::Intrinsic(i) => ("runtime", format!("lean2rust::__runtime::intrinsics::{}", i.symbol)),
                Resolution::User { rust_path, .. } => ("application", rust_path.clone()),
            };
            let req = reqs.get(decl.as_str());
            ExternReportEntry {
                declaration: decl.clone(),
                key,
                resolution: resolution.into(),
                implementation,
                lean_type: req.and_then(|r| r.lean_type.clone()),
                source: req.and_then(|r| describe_source(&r.source, input.local_prefix)),
            }
        })
        .collect()
}

#[derive(Serialize)]
struct SourceReportEntry {
    lean_name: String,
    module: Option<String>,
    file: String,
    start: Option<(u32, u32)>,
    end: Option<(u32, u32)>,
}

fn sources_report(input: &GenInput) -> Vec<SourceReportEntry> {
    input
        .success
        .source_metadata
        .iter()
        .map(|e| SourceReportEntry {
            lean_name: e.name.clone(),
            module: None,
            file: display_path(&e.source.location, input.local_prefix),
            start: e.source.range.map(|r| (r.start.line, r.start.column)),
            end: e.source.range.map(|r| (r.end.line, r.end.column)),
        })
        .collect()
}

#[derive(Serialize)]
struct ManifestExport {
    lean_name: String,
    module: String,
    rust_path: String,
    lean_type: String,
    source: Option<String>,
    trust: lean2rust_protocol::Trust,
}

#[derive(Serialize)]
struct ManifestReport {
    lean_version: String,
    lean_githash: String,
    bir_version: u32,
    generator_version: String,
    runtime_abi: u32,
    modules: Vec<String>,
    declarations: usize,
    exports: Vec<ManifestExport>,
}

fn manifest_report(input: &GenInput, naming: &Naming) -> ManifestReport {
    let paths: HashMap<&str, &str> =
        naming.records.iter().map(|r| (r.lean_name.as_str(), r.rust_path.as_str())).collect();
    let program = &input.success.bir;
    let mut exports: Vec<ManifestExport> = input
        .success
        .interface
        .exports
        .iter()
        .map(|e| ManifestExport {
            lean_name: e.name.clone(),
            module: e.module.clone(),
            rust_path: paths.get(e.name.as_str()).copied().unwrap_or_default().to_owned(),
            lean_type: e.lean_type.clone(),
            source: describe_source(&e.source, input.local_prefix),
            trust: e.trust.clone(),
        })
        .collect();
    exports.sort_by(|a, b| a.lean_name.cmp(&b.lean_name));
    ManifestReport {
        lean_version: input.toolchain.lean_version.clone(),
        lean_githash: input.toolchain.lean_githash.clone(),
        bir_version: program.bir_version,
        generator_version: GENERATOR_VERSION.into(),
        runtime_abi: lean2rust_runtime::ABI_VERSION,
        modules: program.modules.iter().map(|m| m.name.clone()).collect(),
        declarations: program.declarations.len(),
        exports,
    }
}

/// Whether `decl` is an extern declaration.
pub fn is_extern(decl: &Declaration) -> bool {
    matches!(decl.body, Body::Extern { .. })
}
