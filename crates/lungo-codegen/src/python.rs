//! The Python binding: a Python package (built with scikit-build-core) whose program is a
//! shared library the package loads with `ctypes`, calling it through the support library
//! `lungo-py` (import `lungo_py`), which carries the runtime.
//!
//! Structures and constructors are frozen dataclasses, an inductive type of several
//! constructors a base class with a subclass per constructor, `Nat` and `Int` `int`,
//! polymorphic types `Generic`. `IO` functions raise `lungo_py.LeanIOError`, `EIO ε` functions
//! `lungo_py.LeanError`, and arguments Lean cannot represent `lungo_py.MalformedError`.

use crate::CodegenError;
use crate::c::boundary::{Boundary, Function};
use crate::core::names::{components, snake_case, upper_camel_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{Distribution, GenerateRequest, Generator, options};
use lungo_runtime::wire::{Field, Returns, Type};
use std::collections::BTreeMap;

/// The Python generator (`--python_out`).
pub struct PythonGenerator;

const PYTHON_KEYWORDS: &[&str] = &[
    "False",
    "None",
    "True",
    "and",
    "as",
    "assert",
    "async",
    "await",
    "break",
    "class",
    "continue",
    "def",
    "del",
    "elif",
    "else",
    "except",
    "finally",
    "for",
    "from",
    "global",
    "if",
    "import",
    "in",
    "is",
    "lambda",
    "nonlocal",
    "not",
    "or",
    "pass",
    "raise",
    "return",
    "try",
    "while",
    "with",
    "yield",
    "match",
    "case",
    "type",
    // Names the generated module uses.
    "lungo_py",
    "os",
    "sys",
    "isinstance",
    "len",
    "tuple",
    "list",
    "bytes",
    "str",
    "int",
    "float",
    "bool",
    "object",
    "self",
    "w",
    "r",
    "v",
    "c",
    "host",
    "args",
    "Host",
    "set_host",
    "run_main",
    "dataclass",
];

fn escape(id: String) -> String {
    if PYTHON_KEYWORDS.contains(&id.as_str()) { format!("{id}_") } else { id }
}

/// A snake_case Python identifier for Lean name components.
fn py_snake(parts: &[String]) -> String {
    escape(parts.iter().map(|p| snake_case(p)).collect::<Vec<_>>().join("_"))
}

fn py_class(parts: &[String]) -> String {
    let id: String = parts.iter().map(|p| upper_camel_case(p)).collect();
    escape(if id.starts_with('_') { format!("X{id}") } else { id })
}

fn py_local(name: &str, index: usize) -> String {
    let s = snake_case(name);
    let id = if name.is_empty() || s.chars().all(|c| c == '_') { format!("x{index}") } else { s };
    let type_param = id.starts_with("type_");
    if type_param { format!("{id}_") } else { escape(id) }
}

fn param_name(i: u32) -> String {
    let letter = (b'A' + (i % 26) as u8) as char;
    if i < 26 { letter.to_string() } else { format!("{letter}{}", i / 26) }
}

struct TypeNames {
    class: String,
    descriptor: String,
    impl_class: String,
    /// Per constructor: its class and fields.
    ctors: Vec<(String, Vec<String>)>,
}

struct Names {
    types: Vec<TypeNames>,
    functions: Vec<String>,
    host_methods: Vec<String>,
}

impl Names {
    fn new(b: &Boundary) -> Result<Names, CodegenError> {
        let mut scope = Scope::new("Python");
        scope.reserve_prefix("_");
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let mut types = Vec::new();
        for ((named, decl), short) in b.types.iter().zip(&b.table.types).zip(short_names(&type_names)) {
            let class = scope.claim(py_class(&short), format!("type {}", named.lean_name))?;
            let descriptor =
                scope.claim(format!("{}_type", py_snake(&short)), format!("the descriptor of {}", named.lean_name))?;
            let mut ctors = Vec::new();
            for c in &decl.ctors {
                let cname = if decl.ctors.len() == 1 {
                    class.clone()
                } else {
                    let last = components(&c.name).last().cloned().unwrap_or_default();
                    scope.claim(format!("{class}{}", py_class(&[last])), format!("constructor {}", c.name))?
                };
                let mut fields = Scope::new("Python");
                let mut fnames = Vec::new();
                for (k, f) in c.fields.iter().enumerate() {
                    fnames.push(fields.claim(py_local(&f.name, k), format!("field {} of {}", f.name, c.name))?);
                }
                ctors.push((cname, fnames));
            }
            types.push(TypeNames { impl_class: format!("_{class}Type"), class, descriptor, ctors });
        }
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(s, f)| scope.claim(py_snake(s), format!("function {}", f.lean_name)))
            .collect::<Result<_, _>>()?;
        let host_names: Vec<&str> = b.host_externs.iter().map(|h| h.declaration.as_str()).collect();
        let mut methods = Scope::new("Python");
        let host_methods = short_names(&host_names)
            .iter()
            .zip(&b.host_externs)
            .map(|(s, h)| methods.claim(py_snake(s), format!("host extern {}", h.declaration)))
            .collect::<Result<_, _>>()?;
        Ok(Names { types, functions, host_methods })
    }
}

fn valid_package(p: &str) -> bool {
    p.starts_with(|c: char| c.is_ascii_lowercase())
        && p.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !PYTHON_KEYWORDS.contains(&p)
}

impl Generator for PythonGenerator {
    fn language(&self) -> &'static str {
        "python"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>> {
        let opts = options(request, "python", &["package", "distribution", "version"]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let package = opts.get("package").map(|s| s.to_string()).unwrap_or_else(|| b.id.to_lowercase());
        if !valid_package(&package) {
            return Err(vec![CodegenError::Configuration(format!(
                "`{package}` cannot name a Python package: use lowercase letters, digits and `_`, starting with a letter (option `package`)"
            ))]);
        }
        let distribution = opts.get("distribution").map(|s| s.to_string()).unwrap_or_else(|| package.replace('_', "-"));
        let version = opts.get("version").copied().unwrap_or("0.1.0");
        let names = Names::new(b).map_err(|e| vec![e])?;
        let mut files = request.program_files.clone();
        let e = Emitter { request, names: &names };
        files.insert(format!("src/{package}/__init__.py"), e.module());
        files.insert(format!("src/{package}/py.typed"), "\n".to_owned());
        files.insert("pyproject.toml".to_owned(), pyproject(request, &package, &distribution, version));
        files.insert("CMakeLists.txt".to_owned(), cmake(request, &package));
        Ok(files)
    }
}

/// The requirement on the support library: exactly this release's, or the local distribution's.
fn support_requirement(request: &GenerateRequest) -> String {
    match &request.runtime.distribution {
        Distribution::Release { .. } => format!("lungo-py=={}", request.runtime.version),
        Distribution::Local { dir } => format!("lungo-py @ {}", file_url(&format!("{}/python", dir.trim_end_matches('/')))),
    }
}

/// The `file:` URL (RFC 8089) of the absolute path `path` (`/` separators): `file:///srv/x`, or
/// `file:///C:/x` for a Windows path, whose drive would otherwise read as the URL's host.
fn file_url(path: &str) -> String {
    let mut url = String::from(if path.starts_with('/') { "file://" } else { "file:///" });
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => url.push(b as char),
            _ => url.push_str(&format!("%{b:02X}")),
        }
    }
    url
}

fn toml_string(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}

fn pyproject(request: &GenerateRequest, package: &str, distribution: &str, version: &str) -> String {
    let support = toml_string(&support_requirement(request));
    format!(
        r#"# Generated by lungo {lungo} from Lean {lean} for program {program}. Do not edit.
[build-system]
requires = ["scikit-build-core>=0.10", {support}]
build-backend = "scikit_build_core.build"

[project]
name = {distribution}
version = {version}
description = {description}
requires-python = ">=3.9"
dependencies = [{support}]

[tool.scikit-build]
minimum-version = "0.10"
wheel.packages = [{src}]
# The program is a shared library loaded with ctypes, not a CPython extension: one wheel per
# platform serves every Python 3.
wheel.py-api = "py3"
cmake.build-type = "Release"
"#,
        lungo = request.runtime.version,
        lean = request.program.lean_version,
        program = request.program.name,
        distribution = toml_string(distribution),
        version = toml_string(version),
        description = toml_string(&format!("The Lean program {} (generated by lungo)", request.program.name)),
        src = toml_string(&format!("src/{package}")),
    )
}

fn cmake(request: &GenerateRequest, package: &str) -> String {
    let b = &request.boundary;
    let mut sources: Vec<&str> =
        request.program_files.keys().filter(|k| k.ends_with(".c")).map(String::as_str).collect();
    sources.sort();
    let mut w = Writer::new();
    w.line(format!(
        "# Generated by lungo {} from Lean {} for program {}. Do not edit.",
        request.runtime.version, request.program.lean_version, request.program.name
    ));
    w.line("cmake_minimum_required(VERSION 3.20)");
    w.line(format!("project({}_lean LANGUAGES C)", b.id));
    w.line("");
    w.line("# The runtime this package runs on is the one lungo_py carries.");
    w.line("find_package(Python COMPONENTS Interpreter REQUIRED)");
    w.line("execute_process(");
    w.line("  COMMAND \"${Python_EXECUTABLE}\" -c \"import lungo_py; print(lungo_py.cmake_dir())\"");
    w.line("  OUTPUT_VARIABLE LUNGO_CMAKE_DIR OUTPUT_STRIP_TRAILING_WHITESPACE COMMAND_ERROR_IS_FATAL ANY)");
    w.line(format!(
        "find_package(lungo {} EXACT CONFIG REQUIRED PATHS \"${{LUNGO_CMAKE_DIR}}\" NO_DEFAULT_PATH)",
        request.runtime.version
    ));
    w.line("");
    w.line(format!("add_library({}_lean SHARED", b.id));
    for s in sources {
        w.line(format!("  {s}"));
    }
    w.line(")");
    w.line(format!("target_include_directories({}_lean PRIVATE program)", b.id));
    w.line(format!("target_compile_features({}_lean PRIVATE c_std_11)", b.id));
    w.line("# The runtime's symbols come from lungo_py's library, loaded first into the process: on");
    w.line("# Windows through its import library, elsewhere resolved when the program is loaded.");
    w.line("if(WIN32)");
    w.line(format!("  target_link_libraries({}_lean PRIVATE lungo::runtime_shared)", b.id));
    w.line("elseif(APPLE)");
    w.line(format!("  target_link_options({}_lean PRIVATE -undefined dynamic_lookup)", b.id));
    w.line("endif()");
    w.line(format!(
        "install(TARGETS {id}_lean LIBRARY DESTINATION {package} RUNTIME DESTINATION {package})",
        id = b.id
    ));
    w.finish()
}

struct Emitter<'a> {
    request: &'a GenerateRequest,
    names: &'a Names,
}

#[derive(Clone, Copy)]
enum Scoped {
    Descriptor,
    Function,
}

fn param_descriptor(i: u32, scoped: Scoped) -> String {
    let n = param_name(i).to_lowercase();
    match scoped {
        Scoped::Descriptor => format!("self.type_{n}"),
        Scoped::Function => format!("type_{n}"),
    }
}

/// A Python string literal.
fn py_string(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}

impl Emitter<'_> {
    fn boundary(&self) -> &Boundary {
        &self.request.boundary
    }

    fn can_be_none(ty: &Type) -> bool {
        matches!(ty, Type::Unit | Type::Option(_))
    }

    /// The type hint of `ty`.
    fn hint(&self, ty: &Type) -> String {
        match ty {
            Type::Nat | Type::Int | Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::UInt64 | Type::USize => {
                "int".into()
            }
            Type::Int8 | Type::Int16 | Type::Int32 | Type::Int64 | Type::ISize => "int".into(),
            Type::Bool => "bool".into(),
            Type::Float | Type::Float32 => "float".into(),
            Type::Char | Type::String => "str".into(),
            Type::Unit => "None".into(),
            Type::ByteArray => "bytes".into(),
            Type::FloatArray => "_t.List[float]".into(),
            Type::Option(t) if Self::can_be_none(t) => format!("_t.Optional[lungo_py.Some[{}]]", self.hint(t)),
            Type::Option(t) => format!("_t.Optional[{}]", self.hint(t)),
            Type::List(t) | Type::Array(t) => format!("_t.List[{}]", self.hint(t)),
            Type::Prod(a, b) => format!("_t.Tuple[{}, {}]", self.hint(a), self.hint(b)),
            Type::Except { error, value } => {
                format!("_t.Union[lungo_py.Ok[{}], lungo_py.Err[{}]]", self.hint(value), self.hint(error))
            }
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.hint(p)).collect();
                format!("_t.Callable[[{}], {}]", ps.join(", "), self.hint(result))
            }
            Type::Param(i) => param_name(*i),
            Type::Inductive { index, args } => {
                let t = &self.names.types[*index as usize];
                if args.is_empty() {
                    t.class.clone()
                } else {
                    format!("{}[{}]", t.class, args.iter().map(|a| self.hint(a)).collect::<Vec<_>>().join(", "))
                }
            }
            Type::Opaque => "lungo_py.Opaque".into(),
        }
    }

    fn descriptor(&self, ty: &Type, scoped: Scoped) -> String {
        let simple = |s: &str| format!("lungo_py.{s}");
        match ty {
            Type::Nat => simple("NAT"),
            Type::Int => simple("INT"),
            Type::Bool => simple("BOOL"),
            Type::UInt8 => simple("UINT8"),
            Type::UInt16 => simple("UINT16"),
            Type::UInt32 => simple("UINT32"),
            Type::UInt64 => simple("UINT64"),
            Type::USize => simple("USIZE"),
            Type::Int8 => simple("INT8"),
            Type::Int16 => simple("INT16"),
            Type::Int32 => simple("INT32"),
            Type::Int64 => simple("INT64"),
            Type::ISize => simple("ISIZE"),
            Type::Float => simple("FLOAT"),
            Type::Float32 => simple("FLOAT32"),
            Type::Char => simple("CHAR"),
            Type::String => simple("STRING"),
            Type::Unit => simple("UNIT"),
            Type::ByteArray => simple("BYTE_ARRAY"),
            Type::FloatArray => simple("FLOAT_ARRAY"),
            Type::Opaque => simple("OPAQUE"),
            Type::Option(t) => format!("lungo_py.option({})", self.descriptor(t, scoped)),
            Type::List(t) => format!("lungo_py.list_of({})", self.descriptor(t, scoped)),
            Type::Array(t) => format!("lungo_py.array_of({})", self.descriptor(t, scoped)),
            Type::Prod(a, b) => {
                format!("lungo_py.pair({}, {})", self.descriptor(a, scoped), self.descriptor(b, scoped))
            }
            Type::Except { error, value } => {
                format!("lungo_py.except_({}, {})", self.descriptor(error, scoped), self.descriptor(value, scoped))
            }
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.descriptor(p, scoped)).collect();
                format!("lungo_py.function([{}], {})", ps.join(", "), self.descriptor(result, scoped))
            }
            Type::Param(i) => param_descriptor(*i, scoped),
            Type::Inductive { index, args } => {
                let ds: Vec<String> = args.iter().map(|a| self.descriptor(a, scoped)).collect();
                format!("{}({})", self.names.types[*index as usize].descriptor, ds.join(", "))
            }
        }
    }

    fn module(&self) -> String {
        let b = self.boundary();
        let r = self.request;
        let mut w = Writer::new();
        w.line(format!(
            "\"\"\"The Lean program {} (generated by lungo {} from Lean {}). Do not edit.\"\"\"",
            r.program.name, r.runtime.version, r.program.lean_version
        ));
        w.line("");
        w.line("from __future__ import annotations");
        w.line("");
        w.line("import os as _os");
        w.line("import sys as _sys");
        w.line("import typing as _t");
        w.line("from dataclasses import dataclass as _dataclass");
        w.line("");
        w.line("import lungo_py");
        w.line("");
        w.line(format!("if lungo_py.__version__ != {}:", py_string(&r.runtime.version)));
        w.line(format!(
            "    raise ImportError(f\"this package needs lungo_py {}, not {{lungo_py.__version__}}\")",
            r.runtime.version
        ));
        w.line("");
        w.line("");
        w.line("def _library() -> str:");
        w.line(format!(
            "    name = {{\"win32\": \"{id}_lean.dll\", \"darwin\": \"lib{id}_lean.dylib\"}}.get(_sys.platform, \"lib{id}_lean.so\")",
            id = b.id
        ));
        w.line("    path = _os.path.join(_os.path.dirname(_os.path.abspath(__file__)), name)");
        w.line("    if not _os.path.isfile(path):");
        w.line("        raise ImportError(f\"the compiled program {path} is missing: install the package with pip\")");
        w.line("    return path");
        w.line("");
        w.line("");
        w.line(format!("_program = lungo_py.Program(_library(), {})", py_string(&b.types_symbol)));
        let max_params = b
            .table
            .types
            .iter()
            .map(|d| d.params)
            .chain(b.functions.iter().map(|f| f.type_params.len() as u32))
            .max()
            .unwrap_or(0);
        if max_params > 0 {
            w.line("");
            for i in 0..max_params {
                w.line(format!("{0} = _t.TypeVar({0:?})", param_name(i)));
            }
        }
        for i in 0..b.types.len() {
            self.named_type(&mut w, i);
        }
        for (f, name) in b.functions.iter().zip(&self.names.functions) {
            self.function(&mut w, f, name);
        }
        self.host(&mut w);
        if let Some(run_main) = &b.run_main {
            w.line("");
            w.line("");
            w.line("def run_main(args: _t.List[str]) -> int:");
            w.line("    \"\"\"Runs the Lean program's `main` with `args`; its exit code.\"\"\"");
            w.line(format!("    return _program.run_main({}, args)", py_string(run_main)));
        }
        let text = w.finish();
        format!("{}\n", text.trim_end())
    }

    fn named_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let decl = &b.table.types[i];
        let tn = &self.names.types[i];
        let n = decl.params;
        let generic = if n == 0 {
            String::new()
        } else {
            format!("_t.Generic[{}]", (0..n).map(param_name).collect::<Vec<_>>().join(", "))
        };
        let applied = if n == 0 {
            tn.class.clone()
        } else {
            format!("{}[{}]", tn.class, (0..n).map(param_name).collect::<Vec<_>>().join(", "))
        };
        let dataclass = |w: &mut Writer, class: &str, base: &str, doc: &str, fields: &[Field], names: &[String]| {
            w.line("");
            w.line("");
            w.line("@_dataclass(frozen=True)");
            w.line(format!("class {class}{}:", if base.is_empty() { String::new() } else { format!("({base})") }));
            w.line(format!("    \"\"\"{doc}\"\"\""));
            for (f, fname) in fields.iter().zip(names) {
                w.line(format!("    {fname}: {}", self.hint(&f.ty)));
            }
        };
        if decl.ctors.len() == 1 {
            dataclass(
                w,
                &tn.class,
                &generic,
                &format!("Lean's {}.", named.lean_name),
                &decl.ctors[0].fields,
                &tn.ctors[0].1,
            );
        } else {
            w.line("");
            w.line("");
            w.line(format!(
                "class {}{}:",
                tn.class,
                if generic.is_empty() { String::new() } else { format!("({generic})") }
            ));
            let ctor_list: Vec<&str> = tn.ctors.iter().map(|c| c.0.as_str()).collect();
            w.line(format!("    \"\"\"Lean's {}: one of {}.\"\"\"", named.lean_name, ctor_list.join(", ")));
            w.line("");
            w.line("    __slots__ = ()");
            for (c, (cname, fnames)) in decl.ctors.iter().zip(&tn.ctors) {
                dataclass(w, cname, &applied, &format!("Lean's {}.", c.name), &c.fields, fnames);
            }
        }
        // The descriptor.
        w.line("");
        w.line("");
        w.line(format!("class {}(lungo_py.Type):", tn.impl_class));
        let params: Vec<String> = (0..n).map(|k| format!("type_{}", param_name(k).to_lowercase())).collect();
        if n > 0 {
            w.line(format!("    def __init__(self, {}):", params.join(", ")));
            for p in &params {
                w.line(format!("        self.{p} = {p}"));
            }
            w.line("");
        }
        let exprs: Vec<String> = params.iter().map(|p| format!(", self.{p}.expr()")).collect();
        w.line("    def expr(self):");
        w.line(format!("        return lungo_py.inductive_expr({i}{})", exprs.join("")));
        w.line("");
        w.line("    def encode(self, w, v):");
        let trivial = decl.trivial.is_some();
        for (ci, (c, (cname, fnames))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
            let keyword = if ci == 0 { "if" } else { "elif" };
            w.line(format!("        {keyword} isinstance(v, {cname}):"));
            if !trivial {
                w.line(format!("            w.u32({ci})"));
            }
            for (f, fname) in c.fields.iter().zip(fnames) {
                w.line(format!("            {}.encode(w, v.{fname})", self.descriptor(&f.ty, Scoped::Descriptor)));
            }
        }
        w.line("        else:");
        w.line(format!("            raise lungo_py.MalformedError(f\"{{v!r}} is not a {}\")", named.lean_name));
        w.line("");
        w.line("    def decode(self, r):");
        if trivial {
            let (cname, _) = &tn.ctors[0];
            let args: Vec<String> = decl.ctors[0]
                .fields
                .iter()
                .map(|f| format!("{}.decode(r)", self.descriptor(&f.ty, Scoped::Descriptor)))
                .collect();
            w.line(format!("        return {cname}({})", args.join(", ")));
        } else {
            w.line("        c = r.u32()");
            for (ci, (c, (cname, _))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                let args: Vec<String> = c
                    .fields
                    .iter()
                    .map(|f| format!("{}.decode(r)", self.descriptor(&f.ty, Scoped::Descriptor)))
                    .collect();
                w.line(format!("        if c == {ci}:"));
                w.line(format!("            return {cname}({})", args.join(", ")));
            }
            w.line(format!(
                "        raise lungo_py.MalformedError(f\"constructor index {{c}} of {}\")",
                named.lean_name
            ));
        }
        w.line("");
        w.line("");
        let typed: Vec<String> = (0..n)
            .map(|k| format!("type_{}: lungo_py.Type[{}]", param_name(k).to_lowercase(), param_name(k)))
            .collect();
        w.line(format!("def {}({}) -> lungo_py.Type[{applied}]:", tn.descriptor, typed.join(", ")));
        w.line(format!("    \"\"\"Describes {} for polymorphic functions.\"\"\"", named.lean_name));
        w.line(format!("    return {}({})", tn.impl_class, params.join(", ")));
    }

    fn returns(&self, r: &Returns) -> String {
        match r {
            Returns::Value(t) => format!("lungo_py.value({})", self.descriptor(t, Scoped::Function)),
            Returns::Io(t) => format!("lungo_py.io({})", self.descriptor(t, Scoped::Function)),
            Returns::Eio { error, value } => format!(
                "lungo_py.eio({}, {})",
                self.descriptor(error, Scoped::Function),
                self.descriptor(value, Scoped::Function)
            ),
        }
    }

    fn result_hint(&self, r: &Returns) -> String {
        match r {
            Returns::Value(t) | Returns::Io(t) | Returns::Eio { value: t, .. } => self.hint(t),
        }
    }

    fn function(&self, w: &mut Writer, f: &Function, name: &str) {
        let n = f.type_params.len() as u32;
        let locals = distinct_locals(f.params.iter().enumerate().map(|(i, p)| py_local(&p.name, i)).collect());
        let mut params: Vec<String> = (0..n)
            .map(|k| format!("type_{}: lungo_py.Type[{}]", param_name(k).to_lowercase(), param_name(k)))
            .collect();
        params.extend(f.params.iter().zip(&locals).map(|(p, l)| format!("{l}: {}", self.hint(&p.ty))));
        w.line("");
        w.line("");
        w.line(format!("def {name}({}) -> {}:", params.join(", "), self.result_hint(&f.returns)));
        w.line(format!(
            "    \"\"\"Lean's {} : {}\"\"\"",
            f.lean_name,
            f.lean_type.replace('\n', " ").replace("\"\"\"", "\\\"\\\"\\\"")
        ));
        let type_args: Vec<String> = (0..n).map(|k| format!("type_{},", param_name(k).to_lowercase())).collect();
        let args: Vec<String> = f
            .params
            .iter()
            .zip(&locals)
            .map(|(p, l)| format!("({}, {l}),", self.descriptor(&p.ty, Scoped::Function)))
            .collect();
        w.line(format!(
            "    return _program.invoke({}, ({}), ({}), {})",
            py_string(&f.symbol),
            type_args.join(" "),
            args.join(" "),
            self.returns(&f.returns)
        ));
    }

    fn host(&self, w: &mut Writer) {
        let b = self.boundary();
        if b.host_externs.is_empty() {
            return;
        }
        w.line("");
        w.line("");
        w.line("class Host(_t.Protocol):");
        w.line("    \"\"\"Implements the program's externs in Python. Methods may run on any thread; an");
        w.line("    exception of a method whose Lean type is not IO or EIO terminates the program.\"\"\"");
        for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
            let locals = distinct_locals(h.params.iter().enumerate().map(|(i, p)| py_local(&p.name, i)).collect());
            let params: Vec<String> =
                h.params.iter().zip(&locals).map(|(p, l)| format!("{l}: {}", self.hint(&p.ty))).collect();
            w.line("");
            let sep = if params.is_empty() { "" } else { ", " };
            w.line(format!("    def {m}(self{sep}{}) -> {}:", params.join(", "), self.result_hint(&h.returns)));
            w.line(format!(
                "        \"\"\"Lean's {} : {}\"\"\"",
                h.declaration,
                h.lean_type.as_deref().unwrap_or("?").replace('\n', " ")
            ));
            w.line("        ...");
        }
        w.line("");
        w.line("");
        w.line("def set_host(host: Host) -> None:");
        w.line("    \"\"\"Installs the implementation of the program's externs; the program's first call");
        w.line("    requires it.\"\"\"");
        for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
            let params: Vec<String> =
                h.params.iter().map(|p| format!("{},", self.descriptor(&p.ty, Scoped::Function))).collect();
            w.line(format!(
                "    _program.set_host_extern({}, {}, ({}), {}, host.{m})",
                py_string(&b.set_host_extern),
                h.index,
                params.join(" "),
                self.returns(&h.returns)
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::file_url;

    #[test]
    fn file_urls_have_an_empty_host_and_escape_what_urls_cannot_hold() {
        assert_eq!(file_url("/srv/lungo dist/python"), "file:///srv/lungo%20dist/python");
        assert_eq!(file_url("C:/Users/Ünï/dist/python"), "file:///C:/Users/%C3%9Cn%C3%AF/dist/python");
        assert_eq!(file_url("/a#b?c%d"), "file:///a%23b%3Fc%25d");
    }
}
