//! The C binding: a C library package over the program, with a typed API on the C value API of
//! `lungo.h`.
//!
//! The package holds the program's C (`program/`), the API (`include/<id>.h`, `src/<id>.c`),
//! and a CMake project building the library `<id>` against the lungo runtime (`lungo::runtime`),
//! which it downloads from the lungo release (verified by SHA-256) or finds installed.
//!
//! Every Lean value is a `lungo_value`; the API adds, per type, its type expression, named
//! constructors and field accessors, and per exported function a C function checking and
//! passing its arguments. Objective-C uses the same API.

use super::boundary::{Boundary, byte_array};
use super::syntax::comment;
use crate::CodegenError;
use crate::core::names::{components, snake_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{Distribution, ExternType, GenerateRequest, Generator, embedded, extern_types, options};
use lungo_runtime::wire::Returns;
use std::collections::BTreeMap;

/// The C generator (`--c_out`).
pub struct CGenerator;

impl Generator for CGenerator {
    fn language(&self) -> &'static str {
        "c"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>> {
        let opts = options(request, "c", &["embed"]).map_err(|e| vec![e])?;
        let embed = embedded(&opts, "C", &[]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let externs = extern_types(request).map_err(|e| vec![e])?;
        let (h, c) = c_api(request, &externs).map_err(|e| vec![e])?;
        let mut files = request.program_files.clone();
        files.insert(format!("include/{}.h", b.id), h);
        files.insert(format!("src/{}.c", b.id), c);
        files.insert("CMakeLists.txt".to_owned(), cmake(request, embed).map_err(|e| vec![e])?);
        Ok(files)
    }
}

/// The C API of the program (`include/<id>.h`, `src/<id>.c`): the header and its source.
///
/// C values are dynamic (`lungo_value`), so a type another C package provides (`externs`, by
/// type index: its header, and its items' prefix `<id>_<type>`) needs nothing of this package but
/// the check that the two were generated for the same layout.
pub(crate) fn c_api(
    request: &GenerateRequest,
    externs: &BTreeMap<usize, &ExternType>,
) -> Result<(String, String), CodegenError> {
    let names = Names::new(&request.boundary)?;
    Ok((header(request, &names), source(request, &names, externs)))
}

/// The name of the macro holding the layout fingerprint of the type whose items are `<prefix>_…`.
fn fingerprint_macro(prefix: &str) -> String {
    format!("{}_FINGERPRINT", prefix.to_uppercase())
}

/// The C names of one constructor.
struct CtorNames {
    /// The constructor function.
    function: String,
    /// The macro of its index.
    index: String,
    /// The field accessors.
    fields: Vec<String>,
    /// The constructor function's parameters.
    params: Vec<String>,
}

struct TypeNames {
    /// The type expression function.
    type_fn: String,
    /// The macro of the type's layout fingerprint.
    fingerprint: String,
    ctors: Vec<CtorNames>,
}

/// The names of the API's items, claimed in one scope: two items with the same name are an
/// error, and no name enters the program's internal namespace (`<id>__`).
struct Names {
    types: Vec<TypeNames>,
    functions: Vec<String>,
    host_externs: Vec<String>,
    initialize: String,
    run_main: Option<String>,
}

fn snake(parts: &[String]) -> String {
    parts.iter().map(|p| snake_case(p)).collect::<Vec<_>>().join("_")
}

/// Names of the generated functions' own locals and parameters, which Lean binders avoid.
const RESERVED_LOCALS: &[&str] = &["args", "types", "fields", "result", "error", "v", "f", "ctx", "drop"];

/// A local C identifier for a Lean binder name: never a C keyword, a type parameter (`t<i>`),
/// or a local of the generated function.
fn local(name: &str, index: usize) -> String {
    let s = snake_case(name);
    let id = if name.is_empty() || s.chars().all(|c| c == '_') { format!("x{index}") } else { super::identifier(&s) };
    let type_param = id.strip_prefix('t').is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()));
    if RESERVED_LOCALS.contains(&id.as_str()) || type_param || C_KEYWORDS.contains(&id.as_str()) {
        format!("{id}_")
    } else {
        id
    }
}

/// C and C++ keywords (the header is also compiled as C++), and names of the C library a
/// parameter would shadow in its macros.
const C_KEYWORDS: &[&str] = &[
    "alignas",
    "alignof",
    "and",
    "and_eq",
    "asm",
    "auto",
    "bitand",
    "bitor",
    "bool",
    "break",
    "case",
    "catch",
    "char",
    "char16_t",
    "char32_t",
    "char8_t",
    "class",
    "compl",
    "concept",
    "const",
    "const_cast",
    "consteval",
    "constexpr",
    "constinit",
    "continue",
    "co_await",
    "co_return",
    "co_yield",
    "decltype",
    "default",
    "delete",
    "do",
    "double",
    "dynamic_cast",
    "else",
    "enum",
    "explicit",
    "export",
    "extern",
    "false",
    "float",
    "for",
    "friend",
    "goto",
    "if",
    "inline",
    "int",
    "long",
    "mutable",
    "namespace",
    "new",
    "noexcept",
    "not",
    "not_eq",
    "nullptr",
    "operator",
    "or",
    "or_eq",
    "private",
    "protected",
    "public",
    "register",
    "reinterpret_cast",
    "requires",
    "restrict",
    "return",
    "short",
    "signed",
    "sizeof",
    "static",
    "static_assert",
    "static_cast",
    "struct",
    "switch",
    "template",
    "this",
    "thread_local",
    "throw",
    "true",
    "try",
    "typedef",
    "typeid",
    "typename",
    "union",
    "unsigned",
    "using",
    "virtual",
    "void",
    "volatile",
    "wchar_t",
    "while",
    "xor",
    "xor_eq",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_Bool",
    "_Complex",
    "_Generic",
    "_Imaginary",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "NULL",
    "errno",
    "assert",
];

impl Names {
    fn new(b: &Boundary) -> Result<Names, CodegenError> {
        let id = &b.id;
        let mut scope = Scope::new("C");
        scope.reserve_prefix(format!("{id}__"));
        let upper = id.to_uppercase();
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let short_types = short_names(&type_names);
        let mut types = Vec::new();
        for ((named, decl), short) in b.types.iter().zip(&b.table.types).zip(&short_types) {
            let base = snake(short);
            let type_fn = scope.claim(format!("{id}_{base}_type"), format!("type {}", named.lean_name))?;
            let fingerprint = scope.claim(
                fingerprint_macro(&format!("{id}_{base}")),
                format!("the layout fingerprint of {}", named.lean_name),
            )?;
            let single = decl.ctors.len() == 1;
            let mut ctors = Vec::new();
            for c in &decl.ctors {
                let cname = snake_case(components(&c.name).last().map(String::as_str).unwrap_or(""));
                let owner = format!("constructor {}", c.name);
                let function = scope.claim(format!("{id}_{base}_{cname}"), owner.clone())?;
                let index = scope.claim(format!("{upper}_{}_{}", base.to_uppercase(), cname.to_uppercase()), owner)?;
                let mut fields = Vec::new();
                for (k, f) in c.fields.iter().enumerate() {
                    let fname = local(&f.name, k);
                    let item =
                        if single { format!("{id}_{base}_{fname}") } else { format!("{id}_{base}_{cname}_{fname}") };
                    fields.push(scope.claim(item, format!("field {} of {}", f.name, c.name))?);
                }
                let params = distinct_locals(c.fields.iter().enumerate().map(|(k, f)| local(&f.name, k)).collect());
                ctors.push(CtorNames { function, index, fields, params });
            }
            types.push(TypeNames { type_fn, fingerprint, ctors });
        }
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(short, f)| scope.claim_function(&f.lean_name, short, |suffix| format!("{id}_{}", snake(suffix))))
            .collect::<Result<_, _>>()?;
        let host_names: Vec<&str> = b.host_externs.iter().map(|h| h.declaration.as_str()).collect();
        let host_externs = short_names(&host_names)
            .iter()
            .zip(&b.host_externs)
            .map(|(short, h)| {
                scope.claim(format!("{id}_implement_{}", snake(short)), format!("host extern {}", h.declaration))
            })
            .collect::<Result<_, _>>()?;
        let initialize = scope.claim(format!("{id}_initialize"), "the program's initializer")?;
        let run_main = match &b.run_main {
            Some(_) => Some(scope.claim(format!("{id}_run_main"), "the program's `main`")?),
            None => None,
        };
        Ok(Names { types, functions, host_externs, initialize, run_main })
    }
}

fn banner(request: &GenerateRequest) -> String {
    comment(&format!(
        "Generated by lungo {} from Lean {} for program {}. Do not edit.",
        request.runtime.version, request.program.lean_version, request.program.name
    ))
}

fn params_list(params: Vec<String>) -> String {
    if params.is_empty() { "void".to_owned() } else { params.join(", ") }
}

/// The parameters of the C function of `f`: type arguments, arguments, result and error.
fn function_params(f: &super::boundary::Function) -> (Vec<String>, Vec<String>) {
    let types: Vec<String> = (0..f.type_params.len()).map(|i| format!("t{i}")).collect();
    let values = distinct_locals(f.params.iter().enumerate().map(|(i, p)| local(&p.name, i)).collect());
    (types, values)
}

fn function_prototype(name: &str, f: &super::boundary::Function) -> String {
    let (types, values) = function_params(f);
    let mut ps: Vec<String> = types.iter().map(|t| format!("const lungo_type *{t}")).collect();
    ps.extend(values.iter().map(|v| format!("const lungo_value *{v}")));
    ps.push("lungo_value **result".to_owned());
    ps.push("lungo_error **error".to_owned());
    format!("int32_t {name}({})", ps.join(", "))
}

fn type_fn_prototype(name: &str, params: usize) -> String {
    let ps: Vec<String> = (0..params).map(|i| format!("const lungo_type *t{i}")).collect();
    format!("lungo_type *{name}({})", params_list(ps))
}

fn describe_returns(r: &Returns) -> &'static str {
    match r {
        Returns::Value(_) => "Returns LUNGO_OK with the result, or LUNGO_MALFORMED if the arguments do not match.",
        Returns::Io(_) => {
            "Returns LUNGO_OK with the result, LUNGO_FAILED with the IO error, or LUNGO_MALFORMED if the arguments do not match."
        }
        Returns::Eio { .. } => {
            "Returns LUNGO_OK with the result, LUNGO_FAILED with the error value, or LUNGO_MALFORMED if the arguments do not match."
        }
    }
}

fn header(request: &GenerateRequest, names: &Names) -> String {
    let b = &request.boundary;
    let guard = format!("{}_H", b.id.to_uppercase());
    let mut w = Writer::new();
    w.line(banner(request));
    w.line(format!("#ifndef {guard}"));
    w.line(format!("#define {guard}"));
    w.line("");
    w.line("#include \"lungo.h\"");
    w.line("");
    w.line("#ifdef __cplusplus");
    w.line("extern \"C\" {");
    w.line("#endif");
    w.line("");
    w.line(comment("Initializes the program; every function initializes it on first use."));
    w.line(format!("void {}(void);", names.initialize));
    if let Some(run_main) = &names.run_main {
        w.line(comment("Runs the Lean program's `main` with `argc` UTF-8 arguments; returns its exit code."));
        w.line(format!("int32_t {run_main}(size_t argc, const char *const *argv);"));
    }
    w.line("");
    for ((named, decl), tn) in b.types.iter().zip(&b.table.types).zip(&names.types) {
        let params = if named.params.is_empty() { String::new() } else { format!(" ({})", named.params.join(" ")) };
        w.line(comment(&format!("{}{params}", named.lean_name)));
        w.line(format!("#define {} \"{}\"", tn.fingerprint, named.fingerprint));
        w.line(format!("{};", type_fn_prototype(&tn.type_fn, named.params.len())));
        for ((ctor, cn), idx) in decl.ctors.iter().zip(&tn.ctors).zip(0u32..) {
            w.line(format!("#define {} {idx}", cn.index));
            let ps: Vec<String> = cn.params.iter().map(|p| format!("lungo_value *{p}")).collect();
            w.line(comment(&format!("{}; takes ownership of the fields.", ctor.name)));
            w.line(format!("lungo_value *{}({});", cn.function, params_list(ps)));
            for (field, accessor) in ctor.fields.iter().zip(&cn.fields) {
                w.line(comment(&format!("Field {} of {} (borrowed).", field.name, ctor.name)));
                w.line(format!("const lungo_value *{accessor}(const lungo_value *v);"));
            }
        }
        w.line("");
    }
    for (f, name) in b.functions.iter().zip(&names.functions) {
        w.line(comment(&format!("{} : {}", f.lean_name, f.lean_type)));
        if !f.type_params.is_empty() {
            w.line(comment(&format!("Type arguments: {}.", f.type_params.join(", "))));
        }
        w.line(comment(describe_returns(&f.returns)));
        w.line(format!("{};", function_prototype(name, f)));
        w.line("");
    }
    for (h, name) in b.host_externs.iter().zip(&names.host_externs) {
        let ty = h.lean_type.as_deref().unwrap_or("unknown type");
        w.line(comment(&format!(
            "Implements the Lean extern {} : {ty}. Required before the program initializes; `f` may run on any thread.",
            h.declaration
        )));
        w.line(format!("void {name}(lungo_function f, void *ctx, lungo_drop drop);"));
        w.line("");
    }
    w.line("#ifdef __cplusplus");
    w.line("}");
    w.line("#endif");
    w.line("");
    w.line(format!("#endif /* {guard} */"));
    w.finish()
}

fn source(request: &GenerateRequest, names: &Names, externs: &BTreeMap<usize, &ExternType>) -> String {
    let b = &request.boundary;
    let prefix = &b.prefix;
    let mut w = Writer::new();
    w.line(banner(request));
    w.line(format!("#include \"{}.h\"", b.id));
    w.line(format!("#include \"{}_program.h\"", b.id));
    let mut headers: Vec<&str> = externs.values().map(|e| e.package.as_str()).collect();
    headers.sort();
    headers.dedup();
    for h in headers {
        w.line(format!("#include {}", super::syntax::string(h.as_bytes())));
    }
    w.line("");
    for (i, f) in b.functions.iter().enumerate() {
        for l in byte_array(&format!("sig_{i}"), &f.signature().encode()) {
            w.line(l);
        }
    }
    for (i, h) in b.host_externs.iter().enumerate() {
        for l in byte_array(&format!("host_sig_{i}"), &h.signature().encode()) {
            w.line(l);
        }
    }
    w.line("");
    if !externs.is_empty() {
        w.line(comment("The packages providing extern types were generated for the layouts this program was."));
        w.line("static lungo_lazy_bits layouts_checked;");
        w.open("static uint64_t check_layouts(void) {");
        for (&i, ext) in externs {
            let named = &b.types[i];
            w.line(format!(
                "lungo_check_layout({}, {}, \"{}\", {});",
                super::syntax::string(named.lean_name.as_bytes()),
                super::syntax::string(ext.package.as_bytes()),
                named.fingerprint,
                fingerprint_macro(&ext.name)
            ));
        }
        w.line("return 1;");
        w.close("}");
    }
    w.open(format!("void {}(void) {{", names.initialize));
    if !externs.is_empty() {
        w.line("(void)lungo_lazy_bits_get(&layouts_checked, check_layouts);");
    }
    w.line(format!("{}();", b.initialize));
    w.close("}");
    if let (Some(name), Some(inner)) = (&names.run_main, &b.run_main) {
        w.open(format!("int32_t {name}(size_t argc, const char *const *argv) {{"));
        w.line(format!("return {inner}(argc, argv);"));
        w.close("}");
    }
    w.line("");
    for (index, ((named, decl), tn)) in b.types.iter().zip(&b.table.types).zip(&names.types).enumerate() {
        let n = named.params.len();
        w.open(format!("{} {{", type_fn_prototype(&tn.type_fn, n)));
        if n == 0 {
            w.line(format!("return lungo_type_inductive({}(), {index}, NULL, 0);", b.types_symbol));
        } else {
            let args: Vec<String> = (0..n).map(|i| format!("t{i}")).collect();
            w.line(format!("const lungo_type *args[{n}] = {{{}}};", args.join(", ")));
            w.line(format!("return lungo_type_inductive({}(), {index}, args, {n});", b.types_symbol));
        }
        w.close("}");
        debug_assert_eq!(decl.ctors.len(), tn.ctors.len());
        for (ctor_index, cn) in tn.ctors.iter().enumerate() {
            let ps: Vec<String> = cn.params.iter().map(|p| format!("lungo_value *{p}")).collect();
            w.open(format!("lungo_value *{}({}) {{", cn.function, params_list(ps)));
            if cn.params.is_empty() {
                w.line(format!("return lungo_value_ctor({ctor_index}, NULL, 0);"));
            } else {
                w.line(format!("lungo_value *fields[{}] = {{{}}};", cn.params.len(), cn.params.join(", ")));
                w.line(format!("return lungo_value_ctor({ctor_index}, fields, {});", cn.params.len()));
            }
            w.close("}");
            for (k, accessor) in cn.fields.iter().enumerate() {
                w.open(format!("const lungo_value *{accessor}(const lungo_value *v) {{"));
                w.line(format!("return lungo_value_ctor_field(v, {ctor_index}, {k});"));
                w.close("}");
            }
        }
        w.line("");
    }
    for (i, (f, name)) in b.functions.iter().zip(&names.functions).enumerate() {
        let (types, values) = function_params(f);
        w.open(format!("{} {{", function_prototype(name, f)));
        if !externs.is_empty() {
            w.line(format!("{}();", names.initialize));
        }
        let type_args = if types.is_empty() {
            "NULL, 0".to_owned()
        } else {
            w.line(format!("const lungo_type *types[{}] = {{{}}};", types.len(), types.join(", ")));
            format!("types, {}", types.len())
        };
        let args = if values.is_empty() {
            "NULL, 0".to_owned()
        } else {
            w.line(format!("const lungo_value *args[{}] = {{{}}};", values.len(), values.join(", ")));
            format!("args, {}", values.len())
        };
        w.line(format!(
            "return lungo_invoke({}(), {}, sig_{i}, sizeof sig_{i}, {type_args}, {args}, result, error);",
            b.types_symbol, f.symbol
        ));
        w.close("}");
        w.line("");
    }
    for (i, (h, name)) in b.host_externs.iter().zip(&names.host_externs).enumerate() {
        w.open(format!("void {name}(lungo_function f, void *ctx, lungo_drop drop) {{"));
        w.line(format!(
            "{prefix}set_host_extern({}, lungo_host_function_new({}(), host_sig_{i}, sizeof host_sig_{i}, f, ctx, drop));",
            h.index, b.types_symbol
        ));
        w.close("}");
        w.line("");
    }
    w.finish()
}

/// A CMake string literal.
fn cmake_string(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' | '\\' | '$' | ';' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The CMake project of the package: its own project, which gets the runtime, or, `embedded`, a
/// directory of the host's project (`add_subdirectory`), which provides `lungo::runtime`.
fn cmake(request: &GenerateRequest, embedded: bool) -> Result<String, CodegenError> {
    let b = &request.boundary;
    let id = &b.id;
    let upper = id.to_uppercase();
    let version = &request.runtime.version;
    let mut w = Writer::new();
    w.line(format!(
        "# Generated by lungo {version} from Lean {} for program {}. Do not edit.",
        request.program.lean_version, request.program.name
    ));
    if embedded {
        w.line("# A directory of the host's CMake project (`add_subdirectory`), which provides the lungo");
        w.line(format!("# runtime {version} as the target lungo::runtime."));
        w.open("if(NOT TARGET lungo::runtime)");
        w.line(format!(
            "message(FATAL_ERROR \"{id} needs the lungo runtime {version}: find it (find_package(lungo {version} EXACT CONFIG)) before adding this directory\")"
        ));
        w.close("endif()");
        w.line("");
        library(&mut w, request);
        return Ok(w.finish());
    }
    w.line("cmake_minimum_required(VERSION 3.20)");
    w.line(format!("project({id} LANGUAGES C)"));
    w.line("");
    w.line(format!("set({upper}_LUNGO_VERSION {})", cmake_string(version)));
    w.open("if(NOT TARGET lungo::runtime)");
    let artifacts: Vec<(&String, &crate::plugin::Artifact)> = match &request.runtime.distribution {
        Distribution::Release { artifacts } => artifacts.iter().filter(|(k, _)| k.as_str() != "xcframework").collect(),
        Distribution::Local { .. } => Vec::new(),
    };
    if let Distribution::Local { dir } = &request.runtime.distribution {
        w.line("# Generated with a local lungo distribution: its runtime, for this machine.");
        w.line(format!(
            "find_package(lungo ${{{upper}_LUNGO_VERSION}} EXACT CONFIG REQUIRED PATHS {} NO_DEFAULT_PATH)",
            cmake_string(&format!("{}/runtime", dir.trim_end_matches('/')))
        ));
    } else if artifacts.is_empty() {
        return Err(CodegenError::Configuration(
            "the lungo release lists no runtime archive for any target".to_owned(),
        ));
    } else {
        w.line(format!(
            "option({upper}_FETCH_LUNGO \"Download the lungo runtime release (otherwise find an installed one)\" ON)"
        ));
        w.open(format!("if({upper}_FETCH_LUNGO)"));
        w.open(format!("if(NOT {upper}_LUNGO_TARGET)"));
        for l in [
            "string(TOLOWER \"${CMAKE_SYSTEM_PROCESSOR}\" _lungo_arch)",
            "if(APPLE AND CMAKE_OSX_ARCHITECTURES)",
            "  list(LENGTH CMAKE_OSX_ARCHITECTURES _lungo_n)",
            "  if(NOT _lungo_n EQUAL 1)",
            &format!(
                "    message(FATAL_ERROR \"The lungo runtime archive holds one architecture: build one architecture, or set {upper}_LUNGO_TARGET\")"
            ),
            "  endif()",
            "  string(TOLOWER \"${CMAKE_OSX_ARCHITECTURES}\" _lungo_arch)",
            "endif()",
            "if(_lungo_arch MATCHES \"^(x86_64|amd64)$\")",
            "  set(_lungo_arch x86_64)",
            "elseif(_lungo_arch MATCHES \"^(aarch64|arm64)$\")",
            "  set(_lungo_arch aarch64)",
            "else()",
            &format!(
                "  message(FATAL_ERROR \"No lungo runtime for processor ${{CMAKE_SYSTEM_PROCESSOR}}: set {upper}_LUNGO_TARGET\")"
            ),
            "endif()",
            "if(CMAKE_SYSTEM_NAME STREQUAL \"iOS\")",
            "  if(CMAKE_OSX_SYSROOT MATCHES \"[Ss]imulator\")",
            "    set(_lungo_target ${_lungo_arch}-apple-ios-sim)",
            "  else()",
            "    set(_lungo_target ${_lungo_arch}-apple-ios)",
            "  endif()",
            "elseif(APPLE)",
            "  set(_lungo_target ${_lungo_arch}-apple-darwin)",
            "elseif(WIN32)",
            "  if(MSVC)",
            "    set(_lungo_target ${_lungo_arch}-pc-windows-msvc)",
            "  else()",
            "    set(_lungo_target ${_lungo_arch}-pc-windows-gnu)",
            "  endif()",
            "elseif(CMAKE_SYSTEM_NAME STREQUAL \"Linux\")",
            "  set(_lungo_target ${_lungo_arch}-unknown-linux-gnu)",
            "else()",
            &format!(
                "  message(FATAL_ERROR \"No lungo runtime for ${{CMAKE_SYSTEM_NAME}}: set {upper}_LUNGO_TARGET\")"
            ),
            "endif()",
            &format!(
                "set({upper}_LUNGO_TARGET ${{_lungo_target}} CACHE STRING \"Target triple of the lungo runtime\")"
            ),
        ] {
            w.line(l);
        }
        w.close("endif()");
        let mut first = true;
        for (triple, artifact) in &artifacts {
            w.line(format!(
                "{}({upper}_LUNGO_TARGET STREQUAL {})",
                if first { "if" } else { "elseif" },
                cmake_string(triple)
            ));
            w.line(format!("  set(_lungo_url {})", cmake_string(&artifact.url)));
            w.line(format!("  set(_lungo_sha256 {})", cmake_string(&artifact.sha256)));
            first = false;
        }
        let available: Vec<&str> = artifacts.iter().map(|(t, _)| t.as_str()).collect();
        w.line("else()");
        w.line(format!(
            "  message(FATAL_ERROR \"lungo ${{{upper}_LUNGO_VERSION}} has no runtime for ${{{upper}_LUNGO_TARGET}} (available: {})\")",
            available.join(", ")
        ));
        w.line("endif()");
        w.line("include(FetchContent)");
        w.line("FetchContent_Declare(lungo_runtime URL ${_lungo_url} URL_HASH SHA256=${_lungo_sha256})");
        w.line("FetchContent_MakeAvailable(lungo_runtime)");
        w.line(format!(
            "find_package(lungo ${{{upper}_LUNGO_VERSION}} EXACT CONFIG REQUIRED PATHS ${{lungo_runtime_SOURCE_DIR}} NO_DEFAULT_PATH)"
        ));
        w.close("else()");
        w.indent();
        w.line(format!("find_package(lungo ${{{upper}_LUNGO_VERSION}} EXACT CONFIG REQUIRED)"));
        w.dedent();
        w.line("endif()");
    }
    w.close("endif()");
    w.line("");
    library(&mut w, request);
    Ok(w.finish())
}

/// The library target of the package.
fn library(w: &mut Writer, request: &GenerateRequest) {
    let id = &request.boundary.id;
    let mut sources: Vec<&str> =
        request.program_files.keys().filter(|k| k.ends_with(".c")).map(String::as_str).collect();
    sources.sort();
    let src = format!("src/{id}.c");
    w.line(format!("add_library({id}"));
    for s in sources.iter().copied().chain(std::iter::once(src.as_str())) {
        w.line(format!("    {s}"));
    }
    w.line(")");
    w.line(format!("add_library({id}::{id} ALIAS {id})"));
    w.line(format!("target_compile_features({id} PUBLIC c_std_11)"));
    w.line(format!(
        "target_include_directories({id} PUBLIC $<BUILD_INTERFACE:${{CMAKE_CURRENT_SOURCE_DIR}}/include> $<BUILD_INTERFACE:${{CMAKE_CURRENT_SOURCE_DIR}}/program>)"
    ));
    w.line(format!("target_link_libraries({id} PUBLIC lungo::runtime)"));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0117: cmake strings cannot expand or split
    #[test]
    fn test0117_cmake_strings_cannot_expand_or_split() {
        assert_eq!(cmake_string("a;b${X}\"\\"), "\"a\\;b\\${X}\\\"\\\\\"");
    }
}
