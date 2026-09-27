//! The Go binding: a Go package that compiles the program with cgo and calls it through the
//! support module `github.com/machinefabric/lungo-go`.
//!
//! Lean's types become Go types: structures are structs, inductive types of several
//! constructors sealed interfaces with a struct per constructor, `Nat` and `Int` `*big.Int`,
//! polymorphic types generic types. Every function returns an error besides its result: the
//! program rejects values Go can express that Lean cannot, and `IO` functions fail with
//! `*lungo.IOError`, `EIO ε` functions with `*lungo.Error[ε]`.

use crate::CodegenError;
use crate::c::boundary::{Boundary, Function, HostExtern};
use crate::core::model::value_recursive;
use crate::core::names::{components, lower_camel_case, upper_camel_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{ExternType, GenerateRequest, Generator, extern_types, options};
use lungo_runtime::wire::{Returns, Type};
use std::collections::BTreeMap;

/// The Go generator (`--go_out`).
pub struct GoGenerator;

/// The Go module of the support library.
pub const SUPPORT_MODULE: &str = "github.com/machinefabric/lungo-go";

const GO_KEYWORDS: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "type",
    "var",
    // Predeclared identifiers a parameter would shadow.
    "nil",
    "true",
    "false",
    "iota",
    "len",
    "cap",
    "make",
    "new",
    "append",
    "copy",
    "delete",
    "panic",
    "recover",
    "print",
    "println",
    "string",
    "int",
    "bool",
    "byte",
    "rune",
    "error",
    "any",
    "complex",
    "real",
    "imag",
    "close",
    "min",
    "max",
    "clear",
    // Imported packages and the generated functions' locals.
    "lungo",
    "big",
    "unsafe",
    "sync",
    "C",
    "w",
    "r",
    "v",
    "x",
    "c",
    "err",
    "status",
    "out",
    "result",
    "t",
    "h",
    "p",
];

impl Generator for GoGenerator {
    fn language(&self) -> &'static str {
        "go"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>> {
        let opts = options(request, "go", &["package"]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let package = match opts.get("package") {
            Some(p) => (*p).to_owned(),
            None => b.id.to_lowercase(),
        };
        let valid = package.starts_with(|c: char| c.is_ascii_lowercase())
            && package.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !GO_KEYWORDS[..25].contains(&package.as_str());
        if !valid {
            return Err(vec![CodegenError::Configuration(format!(
                "`{package}` cannot name a Go package: use lowercase letters, digits and `_`, starting with a letter (option `package`)"
            ))]);
        }
        let externs = extern_types(request).map_err(|e| vec![e])?;
        let aliases = import_aliases(&externs).map_err(|e| vec![e])?;
        let names = Names::new(b, &externs, &aliases).map_err(|e| vec![e])?;
        let mut files = BTreeMap::new();
        for (path, text) in &request.program_files {
            files.insert(flatten(path), text.clone());
        }
        let emitter = Emitter {
            request,
            names: &names,
            recursive: value_recursive(&b.table),
            externs: &externs,
            aliases: &aliases,
        };
        let source = emitter.package(&package);
        files.insert(format!("{}.go", b.id.to_lowercase()), source);
        Ok(files)
    }
}

/// The package directory holds the program's C files, which cgo compiles only there. Module
/// files are named `module_<stem>_lean.c`, whose last `_` part is never a Go platform name.
fn flatten(path: &str) -> String {
    let rel = path.strip_prefix("program/").unwrap_or(path);
    match rel.strip_prefix("modules/").and_then(|m| m.strip_suffix(".c")) {
        Some(stem) => format!("module_{stem}_lean.c"),
        None => rel.to_owned(),
    }
}

/// The import alias of every package providing an extern type: the package's last path element,
/// made a Go identifier, and distinct from the generated file's own names and imports.
fn import_aliases(externs: &BTreeMap<usize, &ExternType>) -> Result<BTreeMap<String, String>, CodegenError> {
    let mut packages: Vec<&str> = externs.values().map(|e| e.package.as_str()).collect();
    packages.sort();
    packages.dedup();
    let mut taken: Vec<String> =
        ["lungo", "big", "sync", "unsafe", "utf8", "C", "program", "call", "programOnce", "theProgram"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    let mut out = BTreeMap::new();
    for p in packages {
        let last = p.rsplit('/').next().unwrap_or(p);
        let mut base: String = last
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '_' })
            .collect::<String>()
            .trim_matches('_')
            .to_owned();
        if base.is_empty() || base.starts_with(|c: char| c.is_ascii_digit()) {
            base = format!("pkg{base}");
        }
        if GO_KEYWORDS.contains(&base.as_str()) {
            base.push('_');
        }
        let mut alias = base.clone();
        let mut k = 2;
        while taken.contains(&alias) {
            alias = format!("{base}{k}");
            k += 1;
        }
        taken.push(alias.clone());
        out.insert(p.to_owned(), alias);
    }
    Ok(out)
}

struct TypeNames {
    /// The Go type (interface for several constructors); for an extern type, the providing
    /// package's (`alias.Name`).
    name: String,
    /// The exported constant holding the type's layout fingerprint (none for an extern type,
    /// whose package defines its own).
    fingerprint: Option<String>,
    /// The descriptor function.
    descriptor: String,
    /// The descriptor's implementation type.
    impl_type: String,
    /// The marker method of an interface.
    marker: String,
    /// Per constructor: its struct (the type itself for one constructor) and fields.
    ctors: Vec<(String, Vec<String>)>,
}

struct Names {
    types: Vec<TypeNames>,
    functions: Vec<String>,
    host_methods: Vec<String>,
}

fn go_exported(s: &str) -> String {
    let id = upper_camel_case(s);
    if id.starts_with('_') { format!("X{id}") } else { id }
}

impl Names {
    fn new(
        b: &Boundary,
        externs: &BTreeMap<usize, &ExternType>,
        aliases: &BTreeMap<String, String>,
    ) -> Result<Names, CodegenError> {
        let mut scope = Scope::new("Go");
        for reserved in ["Host", "SetHost", "RunMain"] {
            scope.claim(reserved.to_owned(), "a generated function")?;
        }
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let mut types = Vec::new();
        for (index, ((named, decl), short)) in
            b.types.iter().zip(&b.table.types).zip(short_names(&type_names)).enumerate()
        {
            let own: String = short.iter().map(|c| go_exported(c)).collect();
            if let Some(ext) = externs.get(&index) {
                // The providing package's type, with a descriptor of this program's own (its
                // type expressions index this program's table).
                let descriptor =
                    scope.claim(format!("ext{own}Type"), format!("the descriptor of {}", named.lean_name))?;
                types.push(TypeNames {
                    name: format!("{}.{}", aliases[&ext.package], ext.name),
                    fingerprint: None,
                    impl_type: format!("ext{own}Descriptor"),
                    marker: String::new(),
                    descriptor,
                    ctors: Vec::new(),
                });
                continue;
            }
            let name = scope.claim(own, format!("type {}", named.lean_name))?;
            let descriptor = scope.claim(format!("{name}Type"), format!("the descriptor of {}", named.lean_name))?;
            let fingerprint =
                scope.claim(format!("{name}Fingerprint"), format!("the layout fingerprint of {}", named.lean_name))?;
            let mut ctors = Vec::new();
            for c in &decl.ctors {
                let cname = if decl.ctors.len() == 1 {
                    name.clone()
                } else {
                    let last = components(&c.name).last().cloned().unwrap_or_default();
                    scope.claim(format!("{name}{}", go_exported(&last)), format!("constructor {}", c.name))?
                };
                let mut fields = Scope::new("Go");
                let mut fnames = Vec::new();
                for (k, f) in c.fields.iter().enumerate() {
                    let id = go_field(&f.name, k);
                    fnames.push(fields.claim(id, format!("field {} of {}", f.name, c.name))?);
                }
                ctors.push((cname, fnames));
            }
            let lower = lower_camel_case(&name);
            types.push(TypeNames {
                impl_type: format!("{lower}Descriptor"),
                marker: format!("is{name}"),
                name,
                fingerprint: Some(fingerprint),
                descriptor,
                ctors,
            });
        }
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(s, f)| scope.claim(s.iter().map(|c| go_exported(c)).collect(), format!("function {}", f.lean_name)))
            .collect::<Result<_, _>>()?;
        let host_names: Vec<&str> = b.host_externs.iter().map(|h| h.declaration.as_str()).collect();
        let mut methods = Scope::new("Go");
        let host_methods = short_names(&host_names)
            .iter()
            .zip(&b.host_externs)
            .map(|(s, h)| {
                methods.claim(s.iter().map(|c| go_exported(c)).collect(), format!("host extern {}", h.declaration))
            })
            .collect::<Result<_, _>>()?;
        Ok(Names { types, functions, host_methods })
    }
}

/// An exported struct field.
fn go_field(name: &str, index: usize) -> String {
    let id = upper_camel_case(name);
    if name.is_empty() || id.chars().all(|c| c == '_') {
        format!("F{index}")
    } else if id.starts_with('_') {
        format!("F{id}")
    } else {
        id
    }
}

/// A Go parameter name for a Lean binder.
fn go_local(name: &str, index: usize) -> String {
    let id = lower_camel_case(name);
    let id = if name.is_empty() || id.chars().all(|c| c == '_') { format!("x{index}") } else { id };
    let type_desc = id.starts_with("type") && id.len() > 4;
    if GO_KEYWORDS.contains(&id.as_str()) || type_desc { format!("{id}_") } else { id }
}

/// The name of type parameter `i`: `A`, `B`, … `Z`, `A1`, ….
fn param_name(i: u32) -> String {
    let letter = (b'A' + (i % 26) as u8) as char;
    if i < 26 { letter.to_string() } else { format!("{letter}{}", i / 26) }
}

/// Where type parameters' descriptors are found: in a descriptor's fields, or in a function's
/// parameters.
#[derive(Clone, Copy)]
enum Scoped {
    Descriptor,
    Function,
}

fn param_descriptor(i: u32, scoped: Scoped) -> String {
    match scoped {
        Scoped::Descriptor => format!("t.type{}", param_name(i)),
        Scoped::Function => format!("type{}", param_name(i)),
    }
}

struct Emitter<'a> {
    request: &'a GenerateRequest,
    names: &'a Names,
    recursive: Vec<bool>,
    /// The extern types, by type index, and the import alias of each package providing one.
    externs: &'a BTreeMap<usize, &'a ExternType>,
    aliases: &'a BTreeMap<String, String>,
}

fn is_unit(t: &Type) -> bool {
    matches!(t, Type::Unit)
}

impl Emitter<'_> {
    fn boundary(&self) -> &Boundary {
        &self.request.boundary
    }

    /// The Go type of `ty`.
    fn go_type(&self, ty: &Type) -> String {
        match ty {
            Type::Nat | Type::Int => "*big.Int".into(),
            Type::Bool => "bool".into(),
            Type::UInt8 => "uint8".into(),
            Type::UInt16 => "uint16".into(),
            Type::UInt32 => "uint32".into(),
            Type::UInt64 | Type::USize => "uint64".into(),
            Type::Int8 => "int8".into(),
            Type::Int16 => "int16".into(),
            Type::Int32 => "int32".into(),
            Type::Int64 | Type::ISize => "int64".into(),
            Type::Float => "float64".into(),
            Type::Float32 => "float32".into(),
            Type::Char => "rune".into(),
            Type::String => "string".into(),
            Type::Unit => "lungo.Unit".into(),
            Type::ByteArray => "[]byte".into(),
            Type::FloatArray => "[]float64".into(),
            Type::Option(t) => format!("lungo.Option[{}]", self.go_type(t)),
            Type::List(t) | Type::Array(t) => format!("[]{}", self.go_type(t)),
            Type::Prod(a, b) => format!("lungo.Pair[{}, {}]", self.go_type(a), self.go_type(b)),
            Type::Except { error, value } => format!("lungo.Except[{}, {}]", self.go_type(error), self.go_type(value)),
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.go_type(p)).collect();
                format!("func({}) ({}, error)", ps.join(", "), self.go_type(result))
            }
            Type::Param(i) => param_name(*i),
            Type::Inductive { index, args } => {
                let t = &self.names.types[*index as usize];
                let args = if args.is_empty() {
                    String::new()
                } else {
                    format!("[{}]", args.iter().map(|a| self.go_type(a)).collect::<Vec<_>>().join(", "))
                };
                let ptr = if self.recursive[*index as usize] { "*" } else { "" };
                format!("{ptr}{}{args}", t.name)
            }
            Type::Opaque => "lungo.Opaque".into(),
        }
    }

    /// A Go expression of the descriptor (`lungo.Type[…]`) of `ty`.
    fn descriptor(&self, ty: &Type, scoped: Scoped) -> String {
        let simple = |s: &str| format!("lungo.{s}Type");
        match ty {
            Type::Nat => simple("Nat"),
            Type::Int => simple("Int"),
            Type::Bool => simple("Bool"),
            Type::UInt8 => simple("UInt8"),
            Type::UInt16 => simple("UInt16"),
            Type::UInt32 => simple("UInt32"),
            Type::UInt64 => simple("UInt64"),
            Type::USize => simple("USize"),
            Type::Int8 => simple("Int8"),
            Type::Int16 => simple("Int16"),
            Type::Int32 => simple("Int32"),
            Type::Int64 => simple("Int64"),
            Type::ISize => simple("ISize"),
            Type::Float => simple("Float"),
            Type::Float32 => simple("Float32"),
            Type::Char => simple("Char"),
            Type::String => simple("String"),
            Type::Unit => simple("Unit"),
            Type::ByteArray => simple("ByteArray"),
            Type::FloatArray => simple("FloatArray"),
            Type::Opaque => simple("Opaque"),
            Type::Option(t) => format!("lungo.OptionType({})", self.descriptor(t, scoped)),
            Type::List(t) => format!("lungo.ListType({})", self.descriptor(t, scoped)),
            Type::Array(t) => format!("lungo.ArrayType({})", self.descriptor(t, scoped)),
            Type::Prod(a, b) => {
                format!("lungo.PairType({}, {})", self.descriptor(a, scoped), self.descriptor(b, scoped))
            }
            Type::Except { error, value } => {
                format!("lungo.ExceptType({}, {})", self.descriptor(error, scoped), self.descriptor(value, scoped))
            }
            Type::Function { params, result } => {
                let mut ds: Vec<String> = params.iter().map(|p| self.descriptor(p, scoped)).collect();
                ds.push(self.descriptor(result, scoped));
                format!("lungo.Func{}Type({})", params.len(), ds.join(", "))
            }
            Type::Param(i) => param_descriptor(*i, scoped),
            Type::Inductive { index, args } => {
                let ds: Vec<String> = args.iter().map(|a| self.descriptor(a, scoped)).collect();
                format!("{}({})", self.names.types[*index as usize].descriptor, ds.join(", "))
            }
        }
    }

    fn type_params(n: u32) -> String {
        if n == 0 { String::new() } else { format!("[{} any]", (0..n).map(param_name).collect::<Vec<_>>().join(", ")) }
    }

    fn type_args(n: u32) -> String {
        if n == 0 { String::new() } else { format!("[{}]", (0..n).map(param_name).collect::<Vec<_>>().join(", ")) }
    }

    fn package(&self, package: &str) -> String {
        let b = self.boundary();
        let r = self.request;
        let mut w = Writer::new();
        w.line(format!(
            "// Code generated by lungo {} from Lean {} for program {}; DO NOT EDIT.",
            r.runtime.version, r.program.lean_version, r.program.name
        ));
        w.line("");
        w.line(format!("// Package {package} calls the Lean program {} (generated by lungo).", r.program.name));
        w.line(format!("package {package}"));
        w.line("");
        w.line("/*");
        w.line("#cgo CFLAGS: -I${SRCDIR}");
        w.line("#include <stdlib.h>");
        w.line(format!("#include \"{}_program.h\"", b.id));
        w.line("");
        w.line(format!(
            "static int32_t {}_go_call(lungo_entry f, const uint8_t *in, size_t n, lungo_buffer *out) {{ return f(in, n, out); }}",
            b.id
        ));
        w.line("*/");
        w.line("import \"C\"");
        w.line("");
        w.line("@IMPORTS@");
        w.line("");
        w.line("// Generated for exactly this version of the support module.");
        w.line(format!("const _ = lungo.EnforceVersion{}", r.runtime.version.replace('.', "_")));
        w.line("");
        w.line("var programOnce sync.Once");
        w.line("var theProgram *lungo.Program");
        w.line("");
        w.line("func program() *lungo.Program {");
        w.line(format!(
            "\tprogramOnce.Do(func() {{ theProgram = lungo.NewProgram(unsafe.Pointer(C.{}())) }})",
            b.types_symbol
        ));
        w.line("\treturn theProgram");
        w.line("}");
        w.line("");
        w.line("func call(entry C.lungo_entry, in []byte) (int32, []byte) {");
        w.line("\tvar buf C.lungo_buffer");
        w.line("\tvar p *C.uint8_t");
        w.line("\tif len(in) > 0 {");
        w.line("\t\tp = (*C.uint8_t)(unsafe.Pointer(&in[0]))");
        w.line("\t}");
        w.line(format!("\tstatus := C.{}_go_call(entry, p, C.size_t(len(in)), &buf)", b.id));
        w.line("\tvar out []byte");
        w.line("\tif buf.len > 0 {");
        w.line("\t\tout = C.GoBytes(unsafe.Pointer(buf.data), C.int(buf.len))");
        w.line("\t}");
        w.line("\tC.lungo_buffer_free(&buf)");
        w.line("\treturn int32(status), out");
        w.line("}");
        w.line("");
        self.extern_checks(&mut w);
        for i in 0..b.types.len() {
            if self.externs.contains_key(&i) {
                self.extern_type(&mut w, i);
            } else if b.types[i].opaque {
                self.opaque_type(&mut w, i);
            } else {
                self.named_type(&mut w, i);
            }
        }
        for (f, name) in b.functions.iter().zip(&self.names.functions) {
            self.function(&mut w, f, name);
        }
        self.host(&mut w);
        self.run_main(&mut w);
        let text = w.finish();
        let mut imports = vec![];
        if text.contains("big.") {
            imports.push("\t\"math/big\"".to_owned());
        }
        imports.push("\t\"sync\"".to_owned());
        if text.contains("utf8.") {
            imports.push("\t\"unicode/utf8\"".to_owned());
        }
        imports.push("\t\"unsafe\"".to_owned());
        imports.push(String::new());
        imports.push(format!("\tlungo \"{SUPPORT_MODULE}\""));
        for (package, alias) in self.aliases {
            imports.push(format!("\t{alias} \"{package}\""));
        }
        let text = text.replacen("@IMPORTS@", &format!("import (\n{}\n)", imports.join("\n")), 1);
        format!("{}\n", text.trim_end())
    }

    /// Refuses to run against a providing package generated for another layout of its type.
    fn extern_checks(&self, w: &mut Writer) {
        if self.externs.is_empty() {
            return;
        }
        let b = self.boundary();
        w.line("// The packages providing this program's extern types were generated for the layouts it was.");
        w.line("func init() {");
        for (&i, ext) in self.externs {
            let alias = &self.aliases[&ext.package];
            w.line(format!("\tif {alias}.{}Fingerprint != \"{}\" {{", ext.name, b.types[i].fingerprint));
            w.line(format!(
                "\t\tpanic(\"{} of {} has another layout than the one this program was generated for: regenerate both from the same Lean definition\")",
                b.types[i].lean_name, ext.package
            ));
            w.line("\t}");
        }
        w.line("}");
        w.line("");
    }

    /// The fingerprint constant of type `i`.
    fn fingerprint_const(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let tn = &self.names.types[i];
        let constant = tn.fingerprint.as_ref().expect("a type of this package has a fingerprint constant");
        w.line(format!(
            "// {constant} is the layout fingerprint of Lean's {}: a package using {} from this one checks it.",
            b.types[i].lean_name, tn.name
        ));
        w.line(format!("const {constant} = \"{}\"", b.types[i].fingerprint));
        w.line("");
    }

    /// A type another package provides: its values are that package's, described here by this
    /// program's own table index.
    fn extern_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let n = b.table.types[i].params;
        let tn = &self.names.types[i];
        let ext = self.externs[&i];
        let alias = &self.aliases[&ext.package];
        let tp = Self::type_params(n);
        let ta = Self::type_args(n);
        let value_type = format!("{}{}{ta}", if self.recursive[i] { "*" } else { "" }, tn.name);
        let fields: Vec<(String, String)> =
            (0..n).map(|k| (format!("type{}", param_name(k)), format!("lungo.Type[{}]", param_name(k)))).collect();
        Self::struct_type(w, &format!("type {}{tp}", tn.impl_type), &fields);
        w.line("");
        let params: Vec<String> =
            (0..n).map(|k| format!("type{} lungo.Type[{}]", param_name(k), param_name(k))).collect();
        let args: Vec<String> = (0..n).map(|k| format!("type{}", param_name(k))).collect();
        let field_args: Vec<String> = (0..n).map(|k| format!("t.type{}", param_name(k))).collect();
        w.line(format!("// {} describes Lean's {}, provided by {}.", tn.descriptor, named.lean_name, ext.package));
        w.line(format!("func {}{tp}({}) lungo.Type[{value_type}] {{", tn.descriptor, params.join(", ")));
        w.line(format!("\treturn {}{ta}{{{}}}", tn.impl_type, args.join(", ")));
        w.line("}");
        w.line("");
        let exprs: Vec<String> = (0..n).map(|k| format!("t.type{}.Expr()", param_name(k))).collect();
        let sep = if exprs.is_empty() { "" } else { ", " };
        w.line(format!(
            "func (t {}{ta}) Expr() []byte {{ return lungo.InductiveExpr({i}{sep}{}) }}",
            tn.impl_type,
            exprs.join(", ")
        ));
        let provided = format!("{alias}.{}Type({})", ext.name, field_args.join(", "));
        w.line(format!("func (t {}{ta}) Encode(w *lungo.Writer, v {value_type}) error {{", tn.impl_type));
        w.line(format!("\treturn {provided}.Encode(w, v)"));
        w.line("}");
        w.line(format!("func (t {}{ta}) Decode(r *lungo.Reader) ({value_type}, error) {{", tn.impl_type));
        w.line(format!("\treturn {provided}.Decode(r)"));
        w.line("}");
        w.line("");
    }

    /// A type whose values cross as handles: a type of its own around `lungo.Opaque`.
    fn opaque_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let tn = &self.names.types[i];
        w.line(format!(
            "// {} is Lean's {}, held by handle: only the program's functions make and read its values.",
            tn.name, named.lean_name
        ));
        w.line(format!("type {} struct{{ lungo.Opaque }}", tn.name));
        w.line("");
        self.fingerprint_const(w, i);
        w.line(format!("type {} struct{{}}", tn.impl_type));
        w.line("");
        w.line(format!("// {} describes {} for polymorphic functions.", tn.descriptor, named.lean_name));
        w.line(format!("func {}() lungo.Type[{}] {{ return {}{{}} }}", tn.descriptor, tn.name, tn.impl_type));
        w.line("");
        w.line(format!("func ({}) Expr() []byte {{ return lungo.InductiveExpr({i}) }}", tn.impl_type));
        w.line(format!("func ({}) Encode(w *lungo.Writer, v {}) error {{", tn.impl_type, tn.name));
        w.line("\treturn lungo.OpaqueType.Encode(w, v.Opaque)");
        w.line("}");
        w.line(format!("func ({}) Decode(r *lungo.Reader) ({}, error) {{", tn.impl_type, tn.name));
        w.line("\to, err := lungo.OpaqueType.Decode(r)");
        w.line(format!("\treturn {}{{o}}, err", tn.name));
        w.line("}");
        w.line("");
    }

    fn named_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let decl = &b.table.types[i];
        let tn = &self.names.types[i];
        let n = decl.params;
        let tp = Self::type_params(n);
        let ta = Self::type_args(n);
        let ptr = self.recursive[i];
        let value_type = format!("{}{}{ta}", if ptr { "*" } else { "" }, tn.name);
        // The Go types.
        if decl.ctors.len() == 1 {
            let (_, fields) = &tn.ctors[0];
            w.line(format!("// {} is Lean's {}.", tn.name, named.lean_name));
            let fs: Vec<(String, String)> =
                decl.ctors[0].fields.iter().zip(fields).map(|(f, n)| (n.clone(), self.go_type(&f.ty))).collect();
            Self::struct_type(w, &format!("type {}{tp}", tn.name), &fs);
        } else {
            w.line(format!(
                "// {} is Lean's {}: one of {}.",
                tn.name,
                named.lean_name,
                tn.ctors.iter().map(|c| c.0.as_str()).collect::<Vec<_>>().join(", ")
            ));
            let marker_param = if n == 0 { String::new() } else { param_name(0) };
            w.line(format!("type {}{tp} interface{{ {}({marker_param}) }}", tn.name, tn.marker));
            for (c, (cname, fields)) in decl.ctors.iter().zip(&tn.ctors) {
                w.line("");
                w.line(format!("// {cname} is Lean's {}.", c.name));
                let fs: Vec<(String, String)> =
                    c.fields.iter().zip(fields).map(|(f, n)| (n.clone(), self.go_type(&f.ty))).collect();
                Self::struct_type(w, &format!("type {cname}{tp}"), &fs);
                w.line("");
                w.line(format!("func ({cname}{ta}) {}({marker_param}) {{}}", tn.marker));
            }
        }
        w.line("");
        self.fingerprint_const(w, i);
        // The descriptor.
        let fields: Vec<(String, String)> =
            (0..n).map(|k| (format!("type{}", param_name(k)), format!("lungo.Type[{}]", param_name(k)))).collect();
        Self::struct_type(w, &format!("type {}{tp}", tn.impl_type), &fields);
        w.line("");
        let params: Vec<String> =
            (0..n).map(|k| format!("type{} lungo.Type[{}]", param_name(k), param_name(k))).collect();
        let args: Vec<String> = (0..n).map(|k| format!("type{}", param_name(k))).collect();
        w.line(format!("// {} describes {} for polymorphic functions.", tn.descriptor, named.lean_name));
        w.line(format!("func {}{tp}({}) lungo.Type[{value_type}] {{", tn.descriptor, params.join(", ")));
        w.line(format!("\treturn {}{ta}{{{}}}", tn.impl_type, args.join(", ")));
        w.line("}");
        w.line("");
        let exprs: Vec<String> = (0..n).map(|k| format!("t.type{}.Expr()", param_name(k))).collect();
        let sep = if exprs.is_empty() { "" } else { ", " };
        w.line(format!(
            "func (t {}{ta}) Expr() []byte {{ return lungo.InductiveExpr({i}{sep}{}) }}",
            tn.impl_type,
            exprs.join(", ")
        ));
        w.line("");
        // Encode.
        w.line(format!("func (t {}{ta}) Encode(w *lungo.Writer, v {value_type}) error {{", tn.impl_type));
        if ptr {
            w.line("\tif v == nil {");
            w.line(format!("\t\treturn lungo.Malformed(\"a nil *{}\")", tn.name));
            w.line("\t}");
        }
        let trivial = decl.trivial;
        if decl.ctors.len() == 1 {
            let deref = if ptr { "(*v)" } else { "v" };
            self.encode_fields(w, &decl.ctors[0].fields, &tn.ctors[0].1, deref, trivial.is_none().then_some(0), "\t");
            w.line("\treturn nil");
        } else {
            w.line("\tswitch x := v.(type) {");
            for (ci, (c, (cname, fnames))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                w.line(format!("\tcase {cname}{ta}:"));
                self.encode_fields(w, &c.fields, fnames, "x", Some(ci as u32), "\t\t");
                w.line("\t\treturn nil");
                w.line(format!("\tcase *{cname}{ta}:"));
                w.line("\t\tif x == nil {");
                w.line(format!("\t\t\treturn lungo.Malformed(\"a nil *{cname}\")"));
                w.line("\t\t}");
                w.line("\t\treturn t.Encode(w, *x)");
            }
            w.line("\t}");
            w.line(format!("\treturn lungo.Malformed(\"a nil {}\")", tn.name));
        }
        w.line("}");
        w.line("");
        // Decode.
        w.line(format!("func (t {}{ta}) Decode(r *lungo.Reader) ({value_type}, error) {{", tn.impl_type));
        if decl.ctors.len() == 1 {
            let c = &decl.ctors[0];
            let fail = if ptr { "nil" } else { "v" };
            w.line(format!("\tvar v {}{ta}", tn.name));
            if trivial.is_none() {
                w.line("\tc, err := r.U32()");
                w.line("\tif err != nil {");
                w.line(format!("\t\treturn {fail}, err"));
                w.line("\t}");
                w.line("\tif c != 0 {");
                w.line(format!(
                    "\t\treturn {fail}, lungo.Malformed(\"constructor index %d of {}\", c)",
                    named.lean_name
                ));
                w.line("\t}");
            }
            self.decode_fields(w, &c.fields, &tn.ctors[0].1, fail, "\t");
            w.line(format!("\treturn {}v, nil", if ptr { "&" } else { "" }));
        } else {
            w.line("\tc, err := r.U32()");
            w.line("\tif err != nil {");
            w.line("\t\treturn nil, err");
            w.line("\t}");
            w.line("\tswitch c {");
            for (ci, (c, (cname, fnames))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                w.line(format!("\tcase {ci}:"));
                w.line(format!("\t\tvar v {cname}{ta}"));
                self.decode_fields(w, &c.fields, fnames, "nil", "\t\t");
                w.line("\t\treturn v, nil");
            }
            w.line("\t}");
            w.line(format!("\treturn nil, lungo.Malformed(\"constructor index %d of {}\", c)", named.lean_name));
        }
        w.line("}");
        w.line("");
    }

    /// Encodes the fields of a constructor value `x`, after its index unless it is a trivial
    /// structure (`index` is `None`).
    fn encode_fields(
        &self,
        w: &mut Writer,
        fields: &[lungo_runtime::wire::Field],
        names: &[String],
        x: &str,
        index: Option<u32>,
        indent: &str,
    ) {
        if let Some(i) = index {
            w.line(format!("{indent}w.U32({i})"));
        }
        for (f, fname) in fields.iter().zip(names) {
            w.line(format!(
                "{indent}if err := {}.Encode(w, {x}.{fname}); err != nil {{",
                self.descriptor(&f.ty, Scoped::Descriptor)
            ));
            w.line(format!("{indent}\treturn err"));
            w.line(format!("{indent}}}"));
        }
    }

    /// Decodes the fields of a constructor into `v`, returning `fail` on an error.
    fn decode_fields(
        &self,
        w: &mut Writer,
        fields: &[lungo_runtime::wire::Field],
        names: &[String],
        fail: &str,
        indent: &str,
    ) {
        for (k, (f, fname)) in fields.iter().zip(names).enumerate() {
            w.line(format!("{indent}f{k}, err := {}.Decode(r)", self.descriptor(&f.ty, Scoped::Descriptor)));
            w.line(format!("{indent}if err != nil {{"));
            w.line(format!("{indent}\treturn {fail}, err"));
            w.line(format!("{indent}}}"));
            w.line(format!("{indent}v.{fname} = f{k}"));
        }
    }

    /// The Go result list of a function returning `returns`, and the result type.
    fn result_type(&self, returns: &Returns) -> Option<String> {
        let t = match returns {
            Returns::Value(t) | Returns::Io(t) | Returns::Eio { value: t, .. } => t,
        };
        (!is_unit(t)).then(|| self.go_type(t))
    }

    fn function(&self, w: &mut Writer, f: &Function, name: &str) {
        let n = f.type_params.len() as u32;
        let tp = Self::type_params(n);
        let locals = distinct_locals(f.params.iter().enumerate().map(|(i, p)| go_local(&p.name, i)).collect());
        let mut params: Vec<String> =
            (0..n).map(|k| format!("type{} lungo.Type[{}]", param_name(k), param_name(k))).collect();
        params.extend(f.params.iter().zip(&locals).map(|(p, l)| format!("{l} {}", self.go_type(&p.ty))));
        let result = self.result_type(&f.returns);
        let results = match &result {
            Some(t) => format!("(result {t}, err error)"),
            None => "(err error)".to_owned(),
        };
        w.line(format!("// {name} is Lean's {} : {}", f.lean_name, f.lean_type.replace('\n', " ")));
        w.line(format!("func {name}{tp}({}) {results} {{", params.join(", ")));
        let type_args: Vec<String> = (0..n).map(|k| format!(", type{}", param_name(k))).collect();
        w.line(format!("\tw := lungo.NewCall(program(){})", type_args.join("")));
        w.line("\tdefer w.Release()");
        for (p, l) in f.params.iter().zip(&locals) {
            w.line(format!("\tif err = {}.Encode(w, {l}); err != nil {{", self.descriptor(&p.ty, Scoped::Function)));
            w.line("\t\treturn");
            w.line("\t}");
        }
        w.line(format!("\tstatus, out := call(C.lungo_entry(C.{}), w.Bytes())", f.symbol));
        let decode = match &f.returns {
            Returns::Value(t) => {
                format!("lungo.DecodeValue(program(), status, out, {})", self.descriptor(t, Scoped::Function))
            }
            Returns::Io(t) => {
                format!("lungo.DecodeIO(program(), status, out, {})", self.descriptor(t, Scoped::Function))
            }
            Returns::Eio { error, value } => format!(
                "lungo.DecodeEIO(program(), status, out, {}, {})",
                self.descriptor(error, Scoped::Function),
                self.descriptor(value, Scoped::Function)
            ),
        };
        if result.is_some() {
            w.line(format!("\treturn {decode}"));
        } else {
            w.line(format!("\t_, err = {decode}"));
            w.line("\treturn");
        }
        w.line("}");
        w.line("");
    }

    fn host(&self, w: &mut Writer) {
        let b = self.boundary();
        if b.host_externs.is_empty() {
            return;
        }
        w.line("// Host implements the program's externs in Go. Its methods may run on any goroutine's");
        w.line("// thread; an error of a method whose Lean type is not IO or EIO terminates the program.");
        w.line("type Host interface {");
        for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
            w.line(format!(
                "\t// {m} implements Lean's {} : {}",
                h.declaration,
                h.lean_type.as_deref().unwrap_or("?").replace('\n', " ")
            ));
            w.line(format!("\t{}", self.host_signature(h, m)));
        }
        w.line("}");
        w.line("");
        w.line("// SetHost installs the implementation of the program's externs; the program's first call");
        w.line("// requires it.");
        w.line("func SetHost(h Host) {");
        w.line("\tif h == nil {");
        w.line("\t\tpanic(\"SetHost(nil)\")");
        w.line("\t}");
        for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
            let locals: Vec<String> = (0..h.params.len()).map(|k| format!("a{k}")).collect();
            w.line(format!("\tC.{}(C.size_t({}), C.uint64_t(lungo.RegisterExtern(program(), func(r *lungo.Reader, w *lungo.Writer) error {{", b.set_host_extern, h.index));
            for (p, l) in h.params.iter().zip(&locals) {
                w.line(format!("\t\t{l}, err := {}.Decode(r)", self.descriptor(&p.ty, Scoped::Function)));
                w.line("\t\tif err != nil {");
                w.line("\t\t\treturn err");
                w.line("\t\t}");
            }
            let call = format!("h.{m}({})", locals.join(", "));
            let (t, write) = match &h.returns {
                Returns::Value(t) => (t, format!("lungo.WriteValue(w, {}, ", self.descriptor(t, Scoped::Function))),
                Returns::Io(t) => (t, format!("lungo.WriteIO(w, {}, ", self.descriptor(t, Scoped::Function))),
                Returns::Eio { error, value } => (
                    value,
                    format!(
                        "lungo.WriteEIO(w, {}, {}, ",
                        self.descriptor(error, Scoped::Function),
                        self.descriptor(value, Scoped::Function)
                    ),
                ),
            };
            if is_unit(t) {
                w.line(format!("\t\treturn {write}lungo.Unit{{}}, {call})"));
            } else {
                w.line(format!("\t\tres, err := {call}"));
                w.line(format!("\t\treturn {write}res, err)"));
            }
            w.line("\t})))");
        }
        w.line("}");
        w.line("");
    }

    fn host_signature(&self, h: &HostExtern, method: &str) -> String {
        let locals = distinct_locals(h.params.iter().enumerate().map(|(i, p)| go_local(&p.name, i)).collect());
        let params: Vec<String> =
            h.params.iter().zip(&locals).map(|(p, l)| format!("{l} {}", self.go_type(&p.ty))).collect();
        match self.result_type(&h.returns) {
            Some(t) => format!("{method}({}) ({t}, error)", params.join(", ")),
            None => format!("{method}({}) error", params.join(", ")),
        }
    }

    fn run_main(&self, w: &mut Writer) {
        let b = self.boundary();
        let Some(run_main) = &b.run_main else { return };
        w.line("// RunMain runs the Lean program's main with args and returns its exit code. The arguments");
        w.line("// must be UTF-8 without NUL.");
        w.line("func RunMain(args []string) (int, error) {");
        w.line("\targv := make([]*C.char, len(args))");
        w.line("\tfor i, a := range args {");
        w.line("\t\tfor _, c := range []byte(a) {");
        w.line("\t\t\tif c == 0 {");
        w.line("\t\t\t\treturn 0, lungo.Malformed(\"argument %d contains NUL\", i)");
        w.line("\t\t\t}");
        w.line("\t\t}");
        w.line("\t\tif !utf8.ValidString(a) {");
        w.line("\t\t\treturn 0, lungo.Malformed(\"argument %d is not UTF-8\", i)");
        w.line("\t\t}");
        w.line("\t\targv[i] = C.CString(a)");
        w.line("\t\tdefer C.free(unsafe.Pointer(argv[i]))");
        w.line("\t}");
        w.line("\tvar p **C.char");
        w.line("\tif len(argv) > 0 {");
        w.line("\t\tp = &argv[0]");
        w.line("\t}");
        w.line(format!("\treturn int(C.{run_main}(C.size_t(len(args)), p)), nil"));
        w.line("}");
        w.line("");
    }

    /// A struct type with `fields` (name, type), aligned as gofmt aligns them.
    fn struct_type(w: &mut Writer, head: &str, fields: &[(String, String)]) {
        if fields.is_empty() {
            w.line(format!("{head} struct{{}}"));
            return;
        }
        let width = fields.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
        w.line(format!("{head} struct {{"));
        for (n, t) in fields {
            w.line(format!("\t{n:width$} {t}"));
        }
        w.line("}");
    }
}
