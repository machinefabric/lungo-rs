//! The Swift binding: a Swift package with a C target (the program, and the C API for
//! Objective-C) and a Swift target (the API), on the support package `lungo-swift` (LungoKit
//! and the runtime XCFramework).
//!
//! Structures are structs (final classes when they contain themselves), inductive types of
//! several constructors `indirect enum`s, `Nat` and `Int` `LungoNat` and `LungoInt`, `Char`
//! `Unicode.Scalar`, polymorphic types generic types. Every function throws: `LungoIOError`
//! (`IO`), `LungoError<ε>` (`EIO ε`), `LungoMalformed` (arguments Lean cannot represent).

use crate::CodegenError;
use crate::c::boundary::{Boundary, Function};
use crate::core::model::value_recursive;
use crate::core::names::{components, lower_camel_case, upper_camel_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{Distribution, GenerateRequest, Generator, options};
use lungo_runtime::wire::{Returns, Type, TypeTable};
use std::collections::BTreeMap;

/// The Swift generator (`--swift_out`).
pub struct SwiftGenerator;

/// The repository of the support package.
pub const SUPPORT_REPOSITORY: &str = "https://github.com/jowharshamshiri/lungo-swift";

const SWIFT_KEYWORDS: &[&str] = &[
    "associatedtype", "class", "deinit", "enum", "extension", "fileprivate", "func", "import", "init", "inout",
    "internal", "let", "open", "operator", "private", "precedencegroup", "protocol", "public", "rethrows", "static",
    "struct", "subscript", "typealias", "var", "break", "case", "catch", "continue", "default", "defer", "do", "else",
    "fallthrough", "for", "guard", "if", "in", "repeat", "return", "throw", "switch", "where", "while", "Any", "as",
    "await", "false", "is", "nil", "self", "Self", "super", "throws", "true", "try", "some", "any", "Type",
    "Protocol", "consume", "copy", "borrowing", "consuming", "package",
];

fn escape(id: String) -> String {
    if SWIFT_KEYWORDS.contains(&id.as_str()) { format!("`{id}`") } else { id }
}

fn swift_type_name(parts: &[String]) -> String {
    let id: String = parts.iter().map(|p| upper_camel_case(p)).collect();
    escape(if id.starts_with('_') { format!("X{id}") } else { id })
}

fn swift_member(parts: &[String]) -> String {
    let joined: String = parts.iter().map(|p| upper_camel_case(p)).collect();
    let mut cs = joined.chars();
    let id = match cs.next() {
        Some(c) if c != '_' => c.to_lowercase().chain(cs).collect(),
        _ => format!("x{joined}"),
    };
    escape(id)
}

fn swift_local(name: &str, index: usize) -> String {
    let id = lower_camel_case(name);
    let id = if name.is_empty() || id.chars().all(|c| c == '_') { format!("x{index}") } else { id };
    // The generated functions' own locals and type descriptor parameters.
    if ["w", "r", "v", "program", "host"].contains(&id.as_str()) || id.starts_with("type") {
        format!("{id}_")
    } else {
        escape(id)
    }
}

fn param_name(i: u32) -> String {
    let letter = (b'A' + (i % 26) as u8) as char;
    if i < 26 { letter.to_string() } else { format!("{letter}{}", i / 26) }
}

struct TypeNames {
    name: String,
    /// Per constructor: its enum case (unused for one constructor) and its fields.
    ctors: Vec<(String, Vec<String>)>,
}

struct Names {
    types: Vec<TypeNames>,
    functions: Vec<String>,
    host_methods: Vec<String>,
}

impl Names {
    fn new(b: &Boundary, module: &str) -> Result<Names, CodegenError> {
        let mut scope = Scope::new("Swift");
        for reserved in [format!("{module}Host"), "setHost".into(), "runMain".into(), "program".into(), "entry".into(), module.into()] {
            scope.claim(reserved, "a generated declaration")?;
        }
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let mut types = Vec::new();
        for ((named, decl), short) in b.types.iter().zip(&b.table.types).zip(short_names(&type_names)) {
            let name = scope.claim(swift_type_name(&short), format!("type {}", named.lean_name))?;
            let mut cases = Scope::new("Swift");
            let mut ctors = Vec::new();
            for c in &decl.ctors {
                let last = components(&c.name).last().cloned().unwrap_or_default();
                let case = cases.claim(swift_member(&[last]), format!("constructor {}", c.name))?;
                let mut fields = Scope::new("Swift");
                let mut fnames = Vec::new();
                for (k, f) in c.fields.iter().enumerate() {
                    fnames.push(fields.claim(swift_local(&f.name, k), format!("field {} of {}", f.name, c.name))?);
                }
                ctors.push((case, fnames));
            }
            types.push(TypeNames { name, ctors });
        }
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(s, f)| scope.claim(swift_member(s), format!("function {}", f.lean_name)))
            .collect::<Result<_, _>>()?;
        let host_names: Vec<&str> = b.host_externs.iter().map(|h| h.declaration.as_str()).collect();
        let mut methods = Scope::new("Swift");
        let host_methods = short_names(&host_names)
            .iter()
            .zip(&b.host_externs)
            .map(|(s, h)| methods.claim(swift_member(s), format!("host extern {}", h.declaration)))
            .collect::<Result<_, _>>()?;
        Ok(Names { types, functions, host_methods })
    }
}

/// For each type of `table`, whether its values can be compared for equality (no functions,
/// opaque values or tuples in them; type parameters conditionally).
fn equatable(table: &TypeTable) -> Vec<bool> {
    fn eq(ty: &Type, decls: &[bool]) -> bool {
        match ty {
            Type::Function { .. } | Type::Opaque | Type::Prod(..) => false,
            Type::Option(t) | Type::List(t) | Type::Array(t) => eq(t, decls),
            Type::Except { error, value } => eq(error, decls) && eq(value, decls),
            Type::Inductive { index, args } => decls[*index as usize] && args.iter().all(|a| eq(a, decls)),
            _ => true,
        }
    }
    let mut out = vec![true; table.types.len()];
    loop {
        let next: Vec<bool> =
            table.types.iter().map(|d| d.ctors.iter().all(|c| c.fields.iter().all(|f| eq(&f.ty, &out)))).collect();
        if next == out {
            return out;
        }
        out = next;
    }
}

impl Generator for SwiftGenerator {
    fn language(&self) -> &'static str {
        "swift"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>> {
        let opts = options(request, "swift", &["module"]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let module = match opts.get("module") {
            Some(m) => (*m).to_owned(),
            None => upper_camel_case(&request.program.name),
        };
        let valid = module.starts_with(|c: char| c.is_ascii_uppercase())
            && module.chars().all(|c| c.is_ascii_alphanumeric())
            && !SWIFT_KEYWORDS.contains(&module.as_str());
        if !valid {
            return Err(vec![CodegenError::Configuration(format!(
                "`{module}` cannot name a Swift module: use ASCII letters and digits, starting with an uppercase letter (option `module`)"
            ))]);
        }
        let names = Names::new(b, &module).map_err(|e| vec![e])?;
        let program_target = format!("{module}Program");
        let mut files = BTreeMap::new();
        for (path, text) in &request.program_files {
            files.insert(format!("Sources/{program_target}/{path}"), text.clone());
        }
        let (api_header, api_source) = crate::c::api::c_api(request).map_err(|e| vec![e])?;
        files.insert(format!("Sources/{program_target}/include/{}.h", b.id), api_header);
        files.insert(format!("Sources/{program_target}/include/lungo.h"), lungo_runtime::header::HEADER.to_owned());
        files.insert(format!("Sources/{program_target}/include/{}_entries.h", b.id), entries_header(request));
        files.insert(format!("Sources/{program_target}/api/{}.c", b.id), api_source);
        let e = Emitter {
            request,
            names: &names,
            module: &module,
            recursive: value_recursive(&b.table),
            equatable: equatable(&b.table),
        };
        files.insert(format!("Sources/{module}/{module}.swift"), e.module(&program_target));
        files.insert("Package.swift".to_owned(), package_manifest(request, &module, &program_target));
        Ok(files)
    }
}

/// The public header of the program's entry points, for Swift.
fn entries_header(request: &GenerateRequest) -> String {
    let b = &request.boundary;
    let guard = format!("{}_ENTRIES_H", b.id.to_uppercase());
    let mut w = Writer::new();
    w.line(format!(
        "/* Generated by lungo {} from Lean {} for program {}. Do not edit. */",
        request.runtime.version, request.program.lean_version, request.program.name
    ));
    w.line(format!("#ifndef {guard}"));
    w.line(format!("#define {guard}"));
    w.line("#include \"lungo.h\"");
    w.line(format!("const lungo_types *{}(void);", b.types_symbol));
    w.line(format!("void {}(size_t index, uint64_t callback);", b.set_host_extern));
    for f in &b.functions {
        w.line(format!("int32_t {}(const uint8_t *input, size_t len, lungo_buffer *out);", f.symbol));
    }
    if let Some(run_main) = &b.run_main {
        w.line(format!("int32_t {run_main}(size_t argc, const char *const *argv);"));
    }
    w.line(format!("#endif /* {guard} */"));
    w.finish()
}

fn swift_string(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}

fn package_manifest(request: &GenerateRequest, module: &str, program_target: &str) -> String {
    let dependency = match &request.runtime.distribution {
        Distribution::Release { .. } => {
            format!(".package(url: {}, exact: {})", swift_string(SUPPORT_REPOSITORY), swift_string(&request.runtime.version))
        }
        Distribution::Local { dir } => {
            format!(".package(path: {})", swift_string(&format!("{}/lungo-swift", dir.trim_end_matches('/'))))
        }
    };
    format!(
        r#"// swift-tools-version:5.9
// Generated by lungo {version} from Lean {lean} for program {program}. Do not edit.
import PackageDescription

let package = Package(
    name: {module_s},
    platforms: [.macOS({macos}), .iOS({ios})],
    products: [
        // The Swift API.
        .library(name: {module_s}, targets: [{module_s}]),
        // The C API, for C and Objective-C.
        .library(name: {program_s}, targets: [{program_s}]),
    ],
    dependencies: [
        {dependency}
    ],
    targets: [
        .target(
            name: {program_s},
            dependencies: [.product(name: "LungoKit", package: "lungo-swift")],
            cSettings: [.headerSearchPath("program")]
        ),
        .target(
            name: {module_s},
            dependencies: [{program_s}, .product(name: "LungoKit", package: "lungo-swift")]
        ),
    ]
)
"#,
        version = request.runtime.version,
        lean = request.program.lean_version,
        program = request.program.name,
        module_s = swift_string(module),
        program_s = swift_string(program_target),
        macos = swift_string(lungo_runtime::header::MACOS_DEPLOYMENT_TARGET),
        ios = swift_string(lungo_runtime::header::IOS_DEPLOYMENT_TARGET),
    )
}

struct Emitter<'a> {
    request: &'a GenerateRequest,
    names: &'a Names,
    /// The Swift module (the host protocol is `<module>Host`: Foundation declares `Host`).
    module: &'a str,
    recursive: Vec<bool>,
    equatable: Vec<bool>,
}

#[derive(Clone, Copy)]
enum Scoped {
    /// In a type's `lungoType`: the parameters `a`, `b`, ….
    Descriptor,
    /// In a function: the parameters `typeA`, `typeB`, ….
    Function,
}

fn param_descriptor(i: u32, scoped: Scoped) -> String {
    match scoped {
        Scoped::Descriptor => param_name(i).to_lowercase(),
        Scoped::Function => format!("type{}", param_name(i)),
    }
}

impl Emitter<'_> {
    fn boundary(&self) -> &Boundary {
        &self.request.boundary
    }

    fn swift_type(&self, ty: &Type) -> String {
        match ty {
            Type::Nat => "LungoNat".into(),
            Type::Int => "LungoInt".into(),
            Type::Bool => "Bool".into(),
            Type::UInt8 => "UInt8".into(),
            Type::UInt16 => "UInt16".into(),
            Type::UInt32 => "UInt32".into(),
            Type::UInt64 | Type::USize => "UInt64".into(),
            Type::Int8 => "Int8".into(),
            Type::Int16 => "Int16".into(),
            Type::Int32 => "Int32".into(),
            Type::Int64 | Type::ISize => "Int64".into(),
            Type::Float => "Double".into(),
            Type::Float32 => "Float".into(),
            Type::Char => "Unicode.Scalar".into(),
            Type::String => "String".into(),
            Type::Unit => "LungoUnit".into(),
            Type::ByteArray => "[UInt8]".into(),
            Type::FloatArray => "[Double]".into(),
            Type::Option(t) => format!("{}?", self.swift_type_atom(t)),
            Type::List(t) | Type::Array(t) => format!("[{}]", self.swift_type(t)),
            Type::Prod(a, b) => format!("({}, {})", self.swift_type(a), self.swift_type(b)),
            Type::Except { error, value } => format!("LungoExcept<{}, {}>", self.swift_type(error), self.swift_type(value)),
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.swift_type(p)).collect();
                format!("({}) throws -> {}", ps.join(", "), self.swift_type(result))
            }
            Type::Param(i) => param_name(*i),
            Type::Inductive { index, args } => {
                let t = &self.names.types[*index as usize];
                if args.is_empty() {
                    t.name.clone()
                } else {
                    format!("{}<{}>", t.name, args.iter().map(|a| self.swift_type(a)).collect::<Vec<_>>().join(", "))
                }
            }
            Type::Opaque => "LungoOpaque".into(),
        }
    }

    /// The type of a parameter: a function is `@escaping` (the runtime may keep it).
    fn param_type(&self, ty: &Type) -> String {
        let t = self.swift_type(ty);
        if matches!(ty, Type::Function { .. }) { format!("@escaping {t}") } else { t }
    }

    /// A type as an operand of `?`.
    fn swift_type_atom(&self, ty: &Type) -> String {
        let t = self.swift_type(ty);
        if matches!(ty, Type::Function { .. }) { format!("({t})") } else { t }
    }

    fn descriptor(&self, ty: &Type, scoped: Scoped) -> String {
        let simple = |s: &str| format!("Lungo.{s}");
        match ty {
            Type::Nat => simple("nat"),
            Type::Int => simple("int"),
            Type::Bool => simple("bool"),
            Type::UInt8 => simple("uint8"),
            Type::UInt16 => simple("uint16"),
            Type::UInt32 => simple("uint32"),
            Type::UInt64 => simple("uint64"),
            Type::USize => simple("usize"),
            Type::Int8 => simple("int8"),
            Type::Int16 => simple("int16"),
            Type::Int32 => simple("int32"),
            Type::Int64 => simple("int64"),
            Type::ISize => simple("isize"),
            Type::Float => simple("float"),
            Type::Float32 => simple("float32"),
            Type::Char => simple("char"),
            Type::String => simple("string"),
            Type::Unit => simple("unit"),
            Type::ByteArray => simple("byteArray"),
            Type::FloatArray => simple("floatArray"),
            Type::Opaque => simple("opaque"),
            Type::Option(t) => format!("Lungo.option({})", self.descriptor(t, scoped)),
            Type::List(t) => format!("Lungo.list({})", self.descriptor(t, scoped)),
            Type::Array(t) => format!("Lungo.array({})", self.descriptor(t, scoped)),
            Type::Prod(a, b) => format!("Lungo.pair({}, {})", self.descriptor(a, scoped), self.descriptor(b, scoped)),
            Type::Except { error, value } => {
                format!("Lungo.except({}, {})", self.descriptor(error, scoped), self.descriptor(value, scoped))
            }
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.descriptor(p, scoped)).collect();
                format!("Lungo.function({}, returns: {})", ps.join(", "), self.descriptor(result, scoped))
            }
            Type::Param(i) => param_descriptor(*i, scoped),
            Type::Inductive { index, args } => {
                let t = &self.names.types[*index as usize];
                if args.is_empty() {
                    format!("{}.lungoType", t.name)
                } else {
                    let ds: Vec<String> = args.iter().map(|a| self.descriptor(a, scoped)).collect();
                    format!("{}.lungoType({})", t.name, ds.join(", "))
                }
            }
        }
    }

    fn module(&self, program_target: &str) -> String {
        let b = self.boundary();
        let r = self.request;
        let mut w = Writer::new();
        w.line(format!(
            "// Generated by lungo {} from Lean {} for program {}. Do not edit.",
            r.runtime.version, r.program.lean_version, r.program.name
        ));
        w.line("");
        w.line("import LungoKit");
        w.line(format!("import {program_target}"));
        w.line("");
        w.line(format!("/// The Lean program {}.", r.program.name));
        w.line(format!("let program = LungoProgram(types: {}())", b.types_symbol));
        w.line("");
        w.line("/// Calls an entry point of the program with `input`; its status and output.");
        w.line("func entry(_ f: @escaping (UnsafePointer<UInt8>?, Int, UnsafeMutablePointer<lungo_buffer>?) -> Int32) -> LungoCall {");
        w.line("    { input, count in");
        w.line("        var buf = lungo_buffer()");
        w.line("        let status = f(input, count, &buf)");
        w.line("        let out = buf.len > 0 ? Array(UnsafeBufferPointer(start: buf.data, count: Int(buf.len))) : []");
        w.line("        lungo_buffer_free(&buf)");
        w.line("        return (status, out)");
        w.line("    }");
        w.line("}");
        for i in 0..b.types.len() {
            self.named_type(&mut w, i);
        }
        for (f, name) in b.functions.iter().zip(&self.names.functions) {
            self.function(&mut w, f, name);
        }
        self.host(&mut w);
        if let Some(run_main) = &b.run_main {
            w.line("");
            w.line("/// Runs the Lean program's `main` with `args`; its exit code.");
            w.line("public func runMain(_ args: [String]) throws -> Int32 {");
            w.line(format!("    try program.runMain({run_main}, args)"));
            w.line("}");
        }
        w.finish()
    }

    fn named_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let decl = &b.table.types[i];
        let tn = &self.names.types[i];
        let n = decl.params;
        let generics = if n == 0 {
            String::new()
        } else {
            format!("<{}>", (0..n).map(param_name).collect::<Vec<_>>().join(", "))
        };
        let applied = format!("{}{generics}", tn.name);
        let eq = self.equatable[i];
        let conformance = |w: &mut Writer| {
            if !eq {
                return;
            }
            if n == 0 {
                w.line(format!("extension {}: Equatable {{}}", tn.name));
            } else {
                let constraints: Vec<String> = (0..n).map(|k| format!("{}: Equatable", param_name(k))).collect();
                w.line(format!("extension {}: Equatable where {} {{}}", tn.name, constraints.join(", ")));
            }
        };
        w.line("");
        if decl.ctors.len() == 1 {
            let c = &decl.ctors[0];
            let (_, fields) = &tn.ctors[0];
            let class = self.recursive[i];
            w.line(format!("/// Lean's {}.", named.lean_name));
            w.line(format!("public {} {}{generics} {{", if class { "final class" } else { "struct" }, tn.name));
            for (f, fname) in c.fields.iter().zip(fields) {
                w.line(format!("    public let {fname}: {}", self.swift_type(&f.ty)));
            }
            let params: Vec<String> =
                c.fields.iter().zip(fields).map(|(f, fname)| format!("{fname}: {}", self.param_type(&f.ty))).collect();
            w.line("");
            w.line(format!("    public init({}) {{", params.join(", ")));
            for fname in fields {
                w.line(format!("        self.{fname} = {fname}"));
            }
            w.line("    }");
            w.line("}");
            if eq {
                w.line("");
                if class {
                    // A class: equality of its fields.
                    let constraints: Vec<String> = (0..n).map(|k| format!("{}: Equatable", param_name(k))).collect();
                    let clause = if n == 0 { String::new() } else { format!(" where {}", constraints.join(", ")) };
                    w.line(format!("extension {}: Equatable{clause} {{", tn.name));
                    w.line(format!("    public static func == (a: {applied}, b: {applied}) -> Bool {{"));
                    let cmp: Vec<String> = fields.iter().map(|f| format!("a.{f} == b.{f}")).collect();
                    w.line(format!("        {}", if cmp.is_empty() { "true".to_owned() } else { cmp.join(" && ") }));
                    w.line("    }");
                    w.line("}");
                } else {
                    conformance(w);
                }
            }
        } else {
            w.line(format!("/// Lean's {}.", named.lean_name));
            w.line(format!("public indirect enum {}{generics} {{", tn.name));
            for (c, (case, fields)) in decl.ctors.iter().zip(&tn.ctors) {
                w.line(format!("    /// Lean's {}.", c.name));
                if c.fields.is_empty() {
                    w.line(format!("    case {case}"));
                } else {
                    let fs: Vec<String> =
                        c.fields.iter().zip(fields).map(|(f, fname)| format!("{fname}: {}", self.swift_type(&f.ty))).collect();
                    w.line(format!("    case {case}({})", fs.join(", ")));
                }
            }
            w.line("}");
            if eq {
                w.line("");
                conformance(w);
            }
        }
        // The descriptor.
        w.line("");
        w.line(format!("extension {} {{", tn.name));
        let params: Vec<String> = (0..n).map(|k| format!("_ {}: LungoType<{}>", param_name(k).to_lowercase(), param_name(k))).collect();
        let exprs: Vec<String> = (0..n).map(|k| format!(", {}.expr", param_name(k).to_lowercase())).collect();
        w.line(format!("    /// Describes {} for polymorphic functions.", named.lean_name));
        if n == 0 {
            w.line(format!("    public static var lungoType: LungoType<{applied}> {{"));
        } else {
            w.line(format!("    public static func lungoType({}) -> LungoType<{applied}> {{", params.join(", ")));
        }
        w.line(format!("        LungoType<{applied}>("));
        w.line(format!("            expr: Lungo.inductiveExpr({i}{}),", exprs.join("")));
        w.line("            encode: { w, v in");
        let trivial = decl.trivial.is_some();
        if decl.ctors.len() == 1 {
            let c = &decl.ctors[0];
            if !trivial {
                w.line("                w.u32(0)");
            }
            for (f, fname) in c.fields.iter().zip(&tn.ctors[0].1) {
                w.line(format!("                try {}.encode(&w, v.{fname})", self.descriptor(&f.ty, Scoped::Descriptor)));
            }
        } else {
            w.line("                switch v {");
            for (ci, (c, (case, fields))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                if c.fields.is_empty() {
                    w.line(format!("                case .{}:", case.trim_matches('`')));
                } else {
                    let binds: Vec<String> = fields.iter().map(|f| format!("let {f}")).collect();
                    w.line(format!("                case .{}({}):", case.trim_matches('`'), binds.join(", ")));
                }
                w.line(format!("                    w.u32({ci})"));
                for (f, fname) in c.fields.iter().zip(fields) {
                    w.line(format!("                    try {}.encode(&w, {fname})", self.descriptor(&f.ty, Scoped::Descriptor)));
                }
            }
            w.line("                }");
        }
        w.line("            },");
        w.line("            decode: { r in");
        let construct = |c: &lungo_runtime::wire::Ctor, fields: &[String], call: &str| -> String {
            let args: Vec<String> = c
                .fields
                .iter()
                .zip(fields)
                .map(|(f, fname)| format!("{fname}: try {}.decode(&r)", self.descriptor(&f.ty, Scoped::Descriptor)))
                .collect();
            if args.is_empty() && call.starts_with('.') { call.to_owned() } else { format!("{call}({})", args.join(", ")) }
        };
        if decl.ctors.len() == 1 {
            let c = &decl.ctors[0];
            if !trivial {
                w.line("                let c = try r.u32()");
                w.line(format!(
                    "                guard c == 0 else {{ throw LungoMalformed(\"constructor index \\(c) of {}\") }}",
                    named.lean_name
                ));
            }
            w.line(format!("                return {}", construct(c, &tn.ctors[0].1, &applied)));
        } else {
            w.line("                switch try r.u32() {");
            for (ci, (c, (case, fields))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                w.line(format!("                case {ci}: return {}", construct(c, fields, &format!(".{}", case.trim_matches('`')))));
            }
            w.line(format!(
                "                case let c: throw LungoMalformed(\"constructor index \\(c) of {}\")",
                named.lean_name
            ));
            w.line("                }");
        }
        w.line("            })");
        w.line("    }");
        w.line("}");
    }

    fn returns(&self, r: &Returns) -> String {
        match r {
            Returns::Value(t) => format!(".value({})", self.descriptor(t, Scoped::Function)),
            Returns::Io(t) => format!(".io({})", self.descriptor(t, Scoped::Function)),
            Returns::Eio { error, value } => {
                format!(".eio({}, {})", self.descriptor(error, Scoped::Function), self.descriptor(value, Scoped::Function))
            }
        }
    }

    fn result_type(&self, r: &Returns) -> Option<String> {
        let t = match r {
            Returns::Value(t) | Returns::Io(t) | Returns::Eio { value: t, .. } => t,
        };
        (!matches!(t, Type::Unit)).then(|| self.swift_type(t))
    }

    fn function(&self, w: &mut Writer, f: &Function, name: &str) {
        let n = f.type_params.len() as u32;
        let generics = if n == 0 {
            String::new()
        } else {
            format!("<{}>", (0..n).map(param_name).collect::<Vec<_>>().join(", "))
        };
        let locals = distinct_locals(f.params.iter().enumerate().map(|(i, p)| swift_local(&p.name, i)).collect());
        let mut params: Vec<String> = (0..n).map(|k| format!("_ type{}: LungoType<{}>", param_name(k), param_name(k))).collect();
        params.extend(f.params.iter().zip(&locals).map(|(p, l)| format!("_ {l}: {}", self.param_type(&p.ty))));
        let result = self.result_type(&f.returns);
        w.line("");
        w.line(format!("/// Lean's {} : {}", f.lean_name, f.lean_type.replace('\n', " ")));
        w.line(format!(
            "public func {name}{generics}({}) throws{} {{",
            params.join(", "),
            result.as_ref().map(|t| format!(" -> {t}")).unwrap_or_default()
        ));
        let type_args: Vec<String> = (0..n).map(|k| format!("type{}.expr", param_name(k))).collect();
        let prefix = if result.is_some() { "try" } else { "_ = try" };
        w.line(format!("    {prefix} program.invoke("));
        w.line(format!("        entry({}), typeArgs: [{}],", f.symbol, type_args.join(", ")));
        if f.params.is_empty() {
            w.line("        args: { _ in },");
        } else {
            w.line("        args: { w in");
            for (p, l) in f.params.iter().zip(&locals) {
                w.line(format!("            try {}.encode(&w, {l})", self.descriptor(&p.ty, Scoped::Function)));
            }
            w.line("        },");
        }
        w.line(format!("        returns: {})", self.returns(&f.returns)));
        w.line("}");
    }

    fn host(&self, w: &mut Writer) {
        let b = self.boundary();
        if b.host_externs.is_empty() {
            return;
        }
        w.line("");
        w.line("/// Implements the program's externs in Swift. Methods may run on any thread; an error of a");
        w.line("/// method whose Lean type is not IO or EIO terminates the program.");
        w.line(format!("public protocol {}Host: AnyObject {{", self.module));
        let mut signatures = Vec::new();
        for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
            let locals = distinct_locals(h.params.iter().enumerate().map(|(i, p)| swift_local(&p.name, i)).collect());
            let params: Vec<String> =
                h.params.iter().zip(&locals).map(|(p, l)| format!("_ {l}: {}", self.param_type(&p.ty))).collect();
            let result = self.result_type(&h.returns);
            w.line(format!("    /// Lean's {} : {}", h.declaration, h.lean_type.as_deref().unwrap_or("?").replace('\n', " ")));
            w.line(format!(
                "    func {m}({}) throws{}",
                params.join(", "),
                result.as_ref().map(|t| format!(" -> {t}")).unwrap_or_default()
            ));
            signatures.push(result.is_some());
        }
        w.line("}");
        w.line("");
        w.line("/// Installs the implementation of the program's externs; the program's first call requires it.");
        w.line(format!("public func setHost(_ host: {}Host) {{", self.module));
        for ((h, m), has_result) in b.host_externs.iter().zip(&self.names.host_methods).zip(signatures) {
            w.line(format!("    program.hostExtern({}, index: {}, returns: {}) {{ r in", b.set_host_extern, h.index, self.returns(&h.returns)));
            let args: Vec<String> = (0..h.params.len()).map(|k| format!("a{k}")).collect();
            for (p, a) in h.params.iter().zip(&args) {
                w.line(format!("        let {a} = try {}.decode(&r)", self.descriptor(&p.ty, Scoped::Function)));
            }
            if has_result {
                w.line(format!("        return try host.{m}({})", args.join(", ")));
            } else {
                w.line(format!("        try host.{m}({})", args.join(", ")));
                w.line("        return LungoUnit()");
            }
            w.line("    }");
        }
        w.line("}");
    }
}
