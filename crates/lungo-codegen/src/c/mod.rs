//! The C backend: the program as C on the lungo runtime's C library.
//!
//! [`generate_program`] translates the Bridge IR into C files that a host's C toolchain compiles
//! and links against the prebuilt runtime library (`liblungo`, header `lungo.h`): one file
//! per Lean module, a header with the program's prototypes, and the program's initializer and
//! entry point. Every symbol carries the program's prefix, so that several programs link into one
//! process. The language bindings (C, Go, Python, Swift, TypeScript) call into the program through
//! its [`boundary`].

pub mod api;
pub mod boundary;
mod program;
mod syntax;

pub use syntax::identifier;

use crate::core::exports::runtime_exports;
use crate::core::externs::{ApplicationExterns, ExternPlan};
use crate::core::names::{mangle, module_file_stem};
use crate::core::writer::Writer;
use crate::{CodegenError, GENERATOR_VERSION, SourceIndex, describe_source};
use lungo_bir::{Declaration, Initializer, IrType};
use lungo_protocol::{Success, WorkerToolchain};
use program::{Emitter, c_params};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use syntax::{c_type, comment, string};

/// The runtime functions a program's WebAssembly module exports for the TypeScript binding.
pub use lungo_runtime::header::WASM_EXPORTS as WASM_RUNTIME_EXPORTS;

/// The directory of the generated program within a generated package.
pub const PROGRAM_DIR: &str = "program";

pub struct ProgramInput<'a> {
    pub success: &'a Success,
    pub toolchain: &'a WorkerToolchain,
    /// The program's name. Its C identifier (the program's *id*) followed by `__` prefixes every
    /// symbol of the program; the C binding's API uses the id followed by `_`.
    pub name: &'a str,
    /// Extern keys the application implements in the host language.
    pub host_externs: &'a BTreeSet<String>,
    /// Path of the Lean project relative to the package, used in source references.
    pub local_prefix: &'a str,
    /// The target triple the program is compiled for; primitives the target lacks are errors.
    pub target: &'a str,
}

/// The generated program.
pub struct Program {
    /// Files keyed by path relative to the package, under [`PROGRAM_DIR`].
    pub files: BTreeMap<String, String>,
    /// The program's C identifier.
    pub id: String,
    /// The symbol prefix (`<id>__`).
    pub prefix: String,
    /// The program's initializer, `void <prefix>initialize(void)`.
    pub initialize: String,
    /// The program's entry point, `int32_t <prefix>run_main(size_t argc, const char *const *argv)`,
    /// when a root module defines `main`.
    pub run_main: Option<String>,
    /// The program's boundary with the language bindings.
    pub boundary: boundary::Boundary,
}

/// The file stem of the program's own files.
fn stem(id: &str) -> String {
    format!("{id}_program")
}

/// Generates the program as C.
pub fn generate_program(input: &ProgramInput) -> Result<Program, Vec<CodegenError>> {
    let program = &input.success.bir;
    lungo_bir::validate(program).map_err(|e| vec![CodegenError::Validation(e)])?;
    let id = identifier(input.name);
    let prefix = format!("{id}__");
    let sources = SourceIndex::new(&input.success.source_metadata, input.local_prefix);
    let host: BTreeMap<String, String> = input.host_externs.iter().map(|k| (k.clone(), k.clone())).collect();
    let hint = |key: &str| {
        format!(
            "Declare it as a host extern (`host-externs = [{key:?}]` in the `[lean]` table of lungo.toml) and implement it in the host language"
        )
    };
    let application = ApplicationExterns { implementations: &host, setting: "host-externs", hint: &hint };
    let externs = ExternPlan::resolve(&program.declarations, &input.success.extern_requirements, &application, &|r| {
        describe_source(&r.source, input.local_prefix)
    })?;
    check_target(&externs, input)?;
    let mut init_values = HashMap::new();
    for m in &program.modules {
        for init in &m.initializers {
            if let Initializer::Value { decl, init_fn } = init {
                init_values.insert(decl.as_str(), init_fn.as_str());
            }
        }
    }
    let emitter = Emitter {
        prefix: &prefix,
        decls: program.declarations.iter().map(|d| (d.name.as_str(), d)).collect(),
        externs: &externs,
        sources: &sources,
        init_values,
    };
    let main = match &input.success.entry_point {
        Some(entry) => Some((
            entry,
            program.declaration(&entry.declaration).ok_or_else(|| {
                vec![CodegenError::internal(format!("the entry point {} is not compiled", entry.declaration))]
            })?,
        )),
        None => None,
    };
    let run_main = main.as_ref().map(|_| format!("{prefix}run_main"));
    let (boundary, boundary_c) = boundary::generate(
        &boundary::BoundaryInput {
            success: input.success,
            program,
            externs: &externs,
            run_main: run_main.clone(),
            id: &id,
            lean_version: &input.toolchain.lean_version,
        },
        &emitter,
    )?;
    let header_name = format!("{}.h", stem(&id));
    let banner = comment(&format!(
        "Generated by lungo {GENERATOR_VERSION} from Lean {} (Bridge IR {}). Do not edit.",
        input.toolchain.lean_version, program.bir_version
    ));
    let mut errors = Vec::new();
    let mut files = BTreeMap::new();

    // The program header: every prototype, for the module files and the boundary.
    let all: Vec<&Declaration> = program.declarations.iter().collect();
    let mut h = Writer::new();
    h.line(&banner);
    let guard = format!("{}_H", stem(&id).to_uppercase());
    h.line(format!("#ifndef {guard}"));
    h.line(format!("#define {guard}"));
    h.line("#include \"lungo.h\"");
    h.line("");
    emitter.declarations(&mut h, &all);
    for decl in program.declarations.iter().filter(|d| crate::is_extern(d)) {
        if let Some(crate::Resolution::Application { .. }) = externs.resolutions.get(&decl.name) {
            h.line(format!("{};", boundary::host_adapter_prototype(&prefix, decl)));
        }
    }
    h.line("");
    h.line(format!("void {prefix}initialize(void);"));
    h.line(format!("const lungo_types *{prefix}types(void);"));
    h.line(format!("void {prefix}set_host_extern(size_t index, uint64_t callback);"));
    h.line(format!("void {prefix}check_host_externs(void);"));
    for f in &boundary.functions {
        h.line(format!("int32_t {}(const uint8_t *input, size_t len, lungo_buffer *out);", f.symbol));
    }
    if let Some(name) = &run_main {
        h.line(format!("int32_t {name}(size_t argc, const char *const *argv);"));
    }
    h.line(format!("#endif /* {guard} */"));
    files.insert(format!("{PROGRAM_DIR}/{header_name}"), h.finish());
    files.insert(format!("{PROGRAM_DIR}/lungo.h"), lungo_runtime::header::HEADER.to_owned());

    // One file per Lean module.
    let mut by_module: BTreeMap<&str, Vec<&Declaration>> = BTreeMap::new();
    for d in &program.declarations {
        by_module.entry(d.module.as_str()).or_default().push(d);
    }
    let mut folded: HashMap<String, &str> = HashMap::new();
    for module in by_module.keys() {
        if let Some(other) = folded.insert(module_file_stem(module).to_lowercase(), module) {
            return Err(vec![CodegenError::internal(format!(
                "Lean modules {other} and {module} differ only in letter case; their generated files would collide"
            ))]);
        }
    }
    for (module, decls) in &by_module {
        let mut w = Writer::new();
        w.line(&banner);
        w.line(comment(&format!("Compiled Lean module {module}.")));
        w.line(format!("#include \"{header_name}\""));
        w.line("");
        for d in decls {
            if let Err(e) = emitter.declaration(&mut w, d) {
                errors.push(e);
            }
        }
        files.insert(format!("{PROGRAM_DIR}/modules/{}.c", module_file_stem(module)), w.finish());
    }

    // The program: runtime exports, initialization, and the entry point.
    let mut w = Writer::new();
    w.line(&banner);
    w.line(format!("#include \"{header_name}\""));
    w.line("");
    if let Err(e) = emit_runtime_exports(&mut w, input, &emitter) {
        errors.push(e);
    }
    emit_initialize(&mut w, input, &emitter);
    if let (Some((entry, main)), Some(name)) = (&main, &run_main) {
        let takes_args = !c_params(&main.params).is_empty();
        w.line(comment("Runs the Lean program's `main` with `argc` UTF-8 arguments; returns its exit code."));
        w.open(format!("int32_t {name}(size_t argc, const char *const *argv) {{"));
        w.line(format!(
            "return lungo_run_main({prefix}initialize, (void *){}, {}, {}, argc, argv);",
            emitter.symbol(&main.name),
            takes_args,
            entry.returns_exit_code
        ));
        w.close("}");
    }
    files.insert(format!("{PROGRAM_DIR}/{}.c", stem(&id)), w.finish());
    files.insert(format!("{PROGRAM_DIR}/{id}_boundary.c"), format!("{banner}\n{boundary_c}"));
    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(Program { files, initialize: format!("{prefix}initialize"), run_main, id, prefix, boundary })
}

/// Every runtime primitive the program uses is available on the target.
fn check_target(externs: &ExternPlan, input: &ProgramInput) -> Result<(), Vec<CodegenError>> {
    let mut errors = Vec::new();
    for (decl, resolution) in &externs.resolutions {
        if let crate::Resolution::Intrinsic(i) = resolution
            && let Some(reason) = lungo_runtime::registry::unavailable_on(i.symbol, input.target)
        {
            errors.push(CodegenError::external(
                crate::ErrorCode::UnsupportedOnTarget,
                format!(
                    "the program uses {decl} (runtime primitive `{}`), which {} does not provide: {reason}",
                    i.symbol, input.target
                ),
            ));
        }
    }
    if errors.is_empty() { Ok(()) } else { Err(errors) }
}

/// Emits wrappers giving the Lean definitions the runtime calls the C signature the runtime
/// uses, reconciling ownership conventions.
fn emit_runtime_exports(w: &mut Writer, input: &ProgramInput, emitter: &Emitter) -> Result<(), CodegenError> {
    use lungo_runtime::registry::Ty;
    for export in runtime_exports(input.success)? {
        let mut sig = Vec::new();
        let mut pre = Vec::new();
        let mut post = Vec::new();
        let mut args = Vec::new();
        for (i, (p, t)) in export.params.iter().enumerate() {
            sig.push(format!("{} a_{i}", c_type(p.ty)));
            if p.ty.is_object() {
                match (t, p.borrow) {
                    (Ty::obj, true) => post.push(format!("lungo_dec(a_{i});")),
                    (Ty::b_obj, false) => pre.push(format!("lungo_inc(a_{i});")),
                    _ => {}
                }
            }
            args.push(format!("a_{i}"));
        }
        let sig = if sig.is_empty() { "void".to_owned() } else { sig.join(", ") };
        w.line(comment(&format!("The runtime calls {} as `{}`.", export.decl.name, export.symbol)));
        w.open(format!("static lungo_obj {}e_{}({sig}) {{", emitter.prefix, mangle(&export.decl.name)));
        for l in pre {
            w.line(l);
        }
        w.line(format!("lungo_obj r = {}({});", emitter.symbol(&export.decl.name), args.join(", ")));
        for l in post {
            w.line(l);
        }
        w.line("return r;");
        w.close("}");
        w.line("");
    }
    Ok(())
}

/// Emits `<prefix>initialize`: registers the runtime exports and runs every module initializer
/// once, in import order, then ends initialization. Concurrent and repeated calls run it once.
fn emit_initialize(w: &mut Writer, input: &ProgramInput, emitter: &Emitter) {
    let prefix = emitter.prefix;
    let program = &input.success.bir;
    w.open(format!("static uint64_t {prefix}initialize_once(void) {{"));
    w.line(comment("A runtime of another C ABI version does not define lungo_abi_v1: linking fails."));
    w.line("if (lungo_abi_v1() != LUNGO_ABI_VERSION) lungo_panic_unreachable();");
    w.line(comment("Initializers may call host externs: every one must be registered."));
    w.line(format!("{prefix}check_host_externs();"));
    if let Ok(exports) = runtime_exports(input.success) {
        for export in exports {
            w.line(format!(
                "lungo_register_export({}, (void *){prefix}e_{});",
                string(export.symbol.as_bytes()),
                mangle(&export.decl.name)
            ));
        }
    }
    for m in &program.modules {
        if m.initializers.is_empty() {
            continue;
        }
        w.line(comment(&m.name));
        for init in &m.initializers {
            match init {
                Initializer::Io(decl) => w.line(format!(
                    "lungo_dec(lungo_check_initializer({}, {}()));",
                    string(decl.as_bytes()),
                    emitter.symbol(decl)
                )),
                Initializer::Value { decl, init_fn } => {
                    let d = emitter.decls[decl.as_str()];
                    let cell = format!("{prefix}I_{}", mangle(decl));
                    w.open("{");
                    w.line(format!(
                        "lungo_obj v = lungo_check_initializer({}, {}());",
                        string(decl.as_bytes()),
                        emitter.symbol(init_fn)
                    ));
                    let bits = match d.result {
                        IrType::Uint8 | IrType::Uint16 => Some("(uint64_t)lungo_unbox(v)".to_owned()),
                        IrType::Uint32 => Some("(uint64_t)lungo_unbox_uint32(v)".to_owned()),
                        IrType::Uint64 => Some("lungo_unbox_uint64(v)".to_owned()),
                        IrType::Usize => Some("(uint64_t)lungo_unbox_usize(v)".to_owned()),
                        IrType::Float => Some("lungo_bits_of_double(lungo_unbox_float(v))".to_owned()),
                        IrType::Float32 => Some("lungo_bits_of_float(lungo_unbox_float32(v))".to_owned()),
                        _ => None,
                    };
                    match bits {
                        Some(bits) => {
                            w.line(format!("lungo_init_bits_set(&{cell}, {bits});"));
                            w.line("lungo_dec(v);");
                        }
                        None => w.line(format!("lungo_init_obj_set(&{cell}, v);")),
                    }
                    w.close("}");
                }
            }
        }
    }
    w.line("lungo_end_initialization();");
    w.line("return 1;");
    w.close("}");
    w.line(format!("static lungo_lazy_bits {prefix}initialized;"));
    w.line(comment("Initializes the program; concurrent and repeated calls initialize it once."));
    w.open(format!("void {prefix}initialize(void) {{"));
    w.line(format!("(void)lungo_lazy_bits_get(&{prefix}initialized, {prefix}initialize_once);"));
    w.close("}");
    w.line("");
}
