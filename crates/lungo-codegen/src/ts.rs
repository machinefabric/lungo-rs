//! The TypeScript binding: an npm package (ES module with type declarations) whose program is a
//! WebAssembly module (`program.wasm`, which the `lungo` command links), running on the support
//! library `lungo-ts`.
//!
//! Structures are plain objects, inductive types of several constructors objects tagged with
//! `kind`, `Nat`, `Int` and 64-bit integers `bigint`, smaller numbers `number`. `load()`
//! instantiates the module and returns an object whose methods are the program's functions; they
//! throw `LeanIOError` (`IO`), `LeanError` (`EIO ε`) and `MalformedError` (arguments Lean cannot
//! represent).
//!
//! The facilities the host provides are given to `load` (`options.facilities`), one object
//! per facility with a method per operation; `load` rejects with `MissingFacilityError` when
//! one is missing. An async export returns a `Promise`: it takes a handler of its async
//! facility, whose methods (returning answers or promises of them) it awaits for each operation
//! the program asks, and an optional `AbortSignal`. `ASSURANCE` is the program's assurance
//! document.

use crate::CodegenError;
use crate::c::boundary::{Boundary, Function, facility_name, operation_name};
use crate::core::names::{components, lower_camel_case, upper_camel_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{Distribution, ExternType, GenerateRequest, Generator, embedded, extern_types, options};
use lungo_runtime::wire::{Returns, Type};
use std::collections::BTreeMap;

/// The TypeScript generator (`--ts_out`).
pub struct TsGenerator;

const JS_RESERVED: &[&str] = &[
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "debugger",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "function",
    "if",
    "import",
    "in",
    "instanceof",
    "new",
    "null",
    "return",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typeof",
    "var",
    "void",
    "while",
    "with",
    "yield",
    "let",
    "static",
    "implements",
    "interface",
    "package",
    "private",
    "protected",
    "public",
    "await",
    "arguments",
    "eval",
    "undefined",
    "NaN",
    "Infinity",
    "L",
    "program",
];

fn escape(id: String) -> String {
    if JS_RESERVED.contains(&id.as_str()) { format!("{id}_") } else { id }
}

fn camel(parts: &[String]) -> String {
    let joined: String = parts.iter().map(|p| upper_camel_case(p)).collect();
    let mut cs = joined.chars();
    let id = match cs.next() {
        Some(c) if c != '_' => c.to_lowercase().chain(cs).collect(),
        _ => format!("x{joined}"),
    };
    escape(id)
}

fn pascal(parts: &[String]) -> String {
    let id: String = parts.iter().map(|p| upper_camel_case(p)).collect();
    escape(if id.starts_with('_') { format!("X{id}") } else { id })
}

fn js_local(name: &str, index: usize) -> String {
    let id = lower_camel_case(name);
    let id = if name.is_empty() || id.chars().all(|c| c == '_') { format!("x{index}") } else { id };
    if id.starts_with("type") || ["w", "r", "v", "options", "host", "handler", "op"].contains(&id.as_str()) {
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
    /// The TypeScript type (for an extern type, the providing module's: `ext<k>.Name`).
    name: String,
    descriptor: String,
    /// Per constructor: its `kind` and fields.
    ctors: Vec<(String, Vec<String>)>,
}

/// The TypeScript names of a facility: its key in `options.facilities`, its interface, and
/// its methods.
struct FacilityNames {
    key: String,
    interface: String,
    methods: Vec<String>,
}

/// The TypeScript names of an async facility: its handler interface, the dispatcher, the
/// methods.
struct AsyncNames {
    handler: String,
    perform: String,
    methods: Vec<String>,
}

struct Names {
    class: String,
    types: Vec<TypeNames>,
    functions: Vec<String>,
    facilities: Vec<FacilityNames>,
    asyncs: Vec<AsyncNames>,
}

/// The namespace alias of every module providing an extern type (`ext<k>`).
fn module_aliases(externs: &BTreeMap<usize, &ExternType>) -> BTreeMap<String, String> {
    let mut modules: Vec<&str> = externs.values().map(|e| e.package.as_str()).collect();
    modules.sort();
    modules.dedup();
    modules.iter().enumerate().map(|(k, m)| (m.to_string(), format!("ext{k}"))).collect()
}

/// Whether values of `ty` hold handles: opaque values and functions are handles of the program
/// that made them, and a TypeScript program runs in a WebAssembly instance of its own, so they
/// cannot pass from one program to another.
fn holds_handles(table: &lungo_runtime::wire::TypeTable, ty: &Type, seen: &mut Vec<u32>) -> bool {
    match ty {
        Type::Opaque | Type::Function { .. } => true,
        Type::Option(t) | Type::List(t) | Type::Array(t) => holds_handles(table, t, seen),
        Type::Prod(a, b) | Type::Except { error: a, value: b } => {
            holds_handles(table, a, seen) || holds_handles(table, b, seen)
        }
        Type::Inductive { index, args } => {
            if args.iter().any(|a| holds_handles(table, a, seen)) {
                return true;
            }
            if seen.contains(index) {
                return false;
            }
            seen.push(*index);
            let decl = &table.types[*index as usize];
            decl.opaque || decl.ctors.iter().any(|c| c.fields.iter().any(|f| holds_handles(table, &f.ty, seen)))
        }
        _ => false,
    }
}

impl Names {
    fn new(
        b: &Boundary,
        class: &str,
        externs: &BTreeMap<usize, &ExternType>,
        aliases: &BTreeMap<String, String>,
    ) -> Result<Names, CodegenError> {
        let mut scope = Scope::new("TypeScript");
        for reserved in [
            class.to_owned(),
            format!("{class}Facilities"),
            "load".into(),
            "LoadOptions".into(),
            "leanTypes".into(),
            "ASSURANCE".into(),
        ] {
            scope.claim(reserved, "a generated declaration")?;
        }
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let mut types = Vec::new();
        for (index, ((named, decl), short)) in
            b.types.iter().zip(&b.table.types).zip(short_names(&type_names)).enumerate()
        {
            if let Some(ext) = externs.get(&index) {
                let ty = lungo_runtime::wire::Type::Inductive { index: index as u32, args: Vec::new() };
                if holds_handles(&b.table, &ty, &mut Vec::new()) {
                    return Err(CodegenError::Configuration(format!(
                        "the TypeScript extern type `{}` holds Lean values by handle, which cannot pass between programs: each TypeScript program runs in a WebAssembly instance of its own",
                        named.lean_name
                    )));
                }
                let descriptor = scope
                    .claim(format!("ext{}Type", pascal(&short)), format!("the descriptor of {}", named.lean_name))?;
                types.push(TypeNames {
                    name: format!("{}.{}", aliases[&ext.package], ext.name),
                    descriptor,
                    ctors: Vec::new(),
                });
                continue;
            }
            let name = scope.claim(pascal(&short), format!("type {}", named.lean_name))?;
            let descriptor =
                scope.claim(format!("{}Type", camel(&short)), format!("the descriptor of {}", named.lean_name))?;
            let mut kinds = Scope::new("TypeScript");
            let mut ctors = Vec::new();
            for c in &decl.ctors {
                let last = components(&c.name).last().cloned().unwrap_or_default();
                let kind =
                    kinds.claim(camel(&[last]).trim_end_matches('_').to_owned(), format!("constructor {}", c.name))?;
                let mut fields = Scope::new("TypeScript");
                if decl.ctors.len() > 1 {
                    fields.claim("kind".into(), "the constructor tag")?;
                }
                let mut fnames = Vec::new();
                for (k, f) in c.fields.iter().enumerate() {
                    let id = lower_camel_case(&f.name);
                    let id = if f.name.is_empty() || id.chars().all(|c| c == '_') { format!("x{k}") } else { id };
                    fnames.push(fields.claim(id, format!("field {} of {}", f.name, c.name))?);
                }
                ctors.push((kind, fnames));
            }
            types.push(TypeNames { name, descriptor, ctors });
        }
        let mut methods = Scope::new("TypeScript");
        methods.claim("runMain".into(), "the program's `main`")?;
        if !b.async_facilities.is_empty() {
            methods.claim("outstanding".into(), "the count of waiting async programs")?;
        }
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(s, f)| {
                methods.claim_function(&f.lean_name, s, |suffix| camel(suffix).trim_end_matches('_').to_owned())
            })
            .collect::<Result<_, _>>()?;
        let mut keys = Scope::new("TypeScript");
        let mut facilities = Vec::new();
        for c in &b.facilities {
            let base = facility_name(&c.id);
            let key =
                keys.claim(camel(&[base.clone()]).trim_end_matches('_').to_owned(), format!("facility {}", c.id))?;
            let interface = scope.claim(format!("{class}{}", pascal(&[base])), format!("facility {}", c.id))?;
            let mut methods = Scope::new("TypeScript");
            let ms = c
                .operations
                .iter()
                .map(|o| {
                    methods.claim(
                        camel(&[operation_name(&o.declaration).to_owned()]).trim_end_matches('_').to_owned(),
                        format!("operation {}", o.declaration),
                    )
                })
                .collect::<Result<_, _>>()?;
            facilities.push(FacilityNames { key, interface, methods: ms });
        }
        let mut asyncs = Vec::new();
        for c in &b.async_facilities {
            let op = types[c.op_type as usize].name.clone();
            let handler = scope.claim(format!("{op}Handler"), format!("the handler of async facility {}", c.id))?;
            let perform = scope.claim(format!("perform{op}"), format!("the dispatcher of async facility {}", c.id))?;
            let mut methods = Scope::new("TypeScript");
            let ms = c
                .operations
                .iter()
                .map(|o| {
                    let last = components(&o.lean_name).last().cloned().unwrap_or_default();
                    methods.claim(camel(&[last]).trim_end_matches('_').to_owned(), format!("operation {}", o.lean_name))
                })
                .collect::<Result<_, _>>()?;
            asyncs.push(AsyncNames { handler, perform, methods: ms });
        }
        Ok(Names { class: class.to_owned(), types, functions, facilities, asyncs })
    }
}

/// A JavaScript string literal.
fn js_string(s: &str) -> String {
    serde_json::to_string(s).expect("strings serialize")
}

fn valid_npm_name(n: &str) -> bool {
    !n.is_empty()
        && n.len() <= 214
        && n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '-' | '.' | '_' | '@' | '/'))
        && !n.starts_with(['.', '_'])
}

impl Generator for TsGenerator {
    fn language(&self) -> &'static str {
        "ts"
    }

    fn generate(&self, request: &GenerateRequest) -> Result<BTreeMap<String, String>, Vec<CodegenError>> {
        let opts = options(request, "ts", &["package", "version", "embed"]).map_err(|e| vec![e])?;
        let embed = embedded(&opts, "TypeScript", &["package", "version"]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let package = opts.get("package").map(|s| s.to_string()).unwrap_or_else(|| request.program.name.to_lowercase());
        if !valid_npm_name(&package) {
            return Err(vec![CodegenError::Configuration(format!(
                "`{package}` cannot name an npm package (option `package`)"
            ))]);
        }
        let version = opts.get("version").copied().unwrap_or("0.1.0");
        let class = upper_camel_case(&request.program.name);
        let externs = extern_types(request).map_err(|e| vec![e])?;
        let aliases = module_aliases(&externs);
        let names = Names::new(b, &class, &externs, &aliases).map_err(|e| vec![e])?;
        let e = Emitter { request, names: &names, externs: &externs, aliases: &aliases };
        let mut files = BTreeMap::new();
        files.insert("index.js".to_owned(), e.javascript());
        files.insert("index.d.ts".to_owned(), e.declarations());
        // Embedded, the module is part of the host's package, whose manifest depends on lungo-ts.
        if !embed {
            files.insert("package.json".to_owned(), package_json(request, &package, version));
        }
        Ok(files)
    }
}

fn package_json(request: &GenerateRequest, package: &str, version: &str) -> String {
    let support = match &request.runtime.distribution {
        Distribution::Release { .. } => request.runtime.version.clone(),
        Distribution::Local { dir } => format!("file:{}/ts", dir.trim_end_matches('/')),
    };
    let json = serde_json::json!({
        "name": package,
        "version": version,
        "description": format!("The Lean program {} (generated by lungo {})", request.program.name, request.runtime.version),
        "type": "module",
        "main": "./index.js",
        "types": "./index.d.ts",
        "exports": { ".": { "types": "./index.d.ts", "default": "./index.js" } },
        "files": ["index.js", "index.d.ts", "program.wasm", "assurance.json"],
        "engines": { "node": ">=20" },
        "dependencies": { "lungo-ts": support },
    });
    let mut s = serde_json::to_string_pretty(&json).expect("JSON");
    s.push('\n');
    s
}

struct Emitter<'a> {
    request: &'a GenerateRequest,
    names: &'a Names,
    /// The extern types, by type index, and the alias of each module providing one.
    externs: &'a BTreeMap<usize, &'a ExternType>,
    aliases: &'a BTreeMap<String, String>,
}

#[derive(Clone, Copy)]
enum Scoped {
    Descriptor,
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

    fn banner(&self) -> String {
        let r = self.request;
        format!(
            "// Generated by lungo {} from Lean {} for program {}. Do not edit.",
            r.runtime.version, r.program.lean_version, r.program.name
        )
    }

    /// The TypeScript type of `ty`.
    fn ts_type(&self, ty: &Type) -> String {
        match ty {
            Type::Nat | Type::Int | Type::UInt64 | Type::USize | Type::Int64 | Type::ISize => "bigint".into(),
            Type::UInt8 | Type::UInt16 | Type::UInt32 | Type::Int8 | Type::Int16 | Type::Int32 => "number".into(),
            Type::Float | Type::Float32 => "number".into(),
            Type::Bool => "boolean".into(),
            Type::Char | Type::String => "string".into(),
            Type::Unit => "null".into(),
            Type::ByteArray => "Uint8Array".into(),
            Type::FloatArray => "number[]".into(),
            Type::Option(t) => format!("L.Option<{}>", self.ts_type(t)),
            Type::List(t) | Type::Array(t) => format!("Array<{}>", self.ts_type(t)),
            Type::Prod(a, b) => format!("[{}, {}]", self.ts_type(a), self.ts_type(b)),
            Type::Except { error, value } => format!("L.Except<{}, {}>", self.ts_type(error), self.ts_type(value)),
            Type::Function { params, result } => {
                let ps: Vec<String> =
                    params.iter().enumerate().map(|(i, p)| format!("a{i}: {}", self.ts_type(p))).collect();
                format!("(({}) => {})", ps.join(", "), self.ts_type(result))
            }
            Type::Param(i) => param_name(*i),
            Type::Inductive { index, args } => {
                let t = &self.names.types[*index as usize];
                if args.is_empty() {
                    t.name.clone()
                } else {
                    format!("{}<{}>", t.name, args.iter().map(|a| self.ts_type(a)).collect::<Vec<_>>().join(", "))
                }
            }
            Type::Opaque => "L.Opaque".into(),
        }
    }

    fn descriptor(&self, ty: &Type, scoped: Scoped) -> String {
        let simple = |s: &str| format!("L.{s}");
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
            Type::Option(t) => format!("L.option({})", self.descriptor(t, scoped)),
            Type::List(t) => format!("L.list({})", self.descriptor(t, scoped)),
            Type::Array(t) => format!("L.array({})", self.descriptor(t, scoped)),
            Type::Prod(a, b) => format!("L.pair({}, {})", self.descriptor(a, scoped), self.descriptor(b, scoped)),
            Type::Except { error, value } => {
                format!("L.except({}, {})", self.descriptor(error, scoped), self.descriptor(value, scoped))
            }
            Type::Function { params, result } => {
                let ps: Vec<String> = params.iter().map(|p| self.descriptor(p, scoped)).collect();
                format!("L.func([{}], {})", ps.join(", "), self.descriptor(result, scoped))
            }
            Type::Param(i) => param_descriptor(*i, scoped),
            Type::Inductive { index, args } => {
                let ds: Vec<String> = args.iter().map(|a| self.descriptor(a, scoped)).collect();
                format!("{}({})", self.names.types[*index as usize].descriptor, ds.join(", "))
            }
        }
    }

    fn returns(&self, r: &Returns) -> String {
        match r {
            Returns::Value(t) => format!("L.value({})", self.descriptor(t, Scoped::Function)),
            Returns::Io(t) => format!("L.io({})", self.descriptor(t, Scoped::Function)),
            Returns::Eio { error, value } => {
                format!(
                    "L.eio({}, {})",
                    self.descriptor(error, Scoped::Function),
                    self.descriptor(value, Scoped::Function)
                )
            }
            Returns::Async { op, value, .. } => format!(
                "L.asyncProgram({}, {})",
                self.descriptor(&Type::Inductive { index: *op, args: Vec::new() }, Scoped::Function),
                self.descriptor(value, Scoped::Function)
            ),
        }
    }

    fn result_type(&self, r: &Returns) -> String {
        match r {
            Returns::Value(t) | Returns::Io(t) | Returns::Eio { value: t, .. } => {
                if matches!(t, Type::Unit) {
                    "void".into()
                } else {
                    self.ts_type(t)
                }
            }
            Returns::Async { value, .. } => {
                format!("Promise<{}>", if matches!(value, Type::Unit) { "void".into() } else { self.ts_type(value) })
            }
        }
    }

    /// The index of the async facility whose operation type is `op`.
    fn async_index(&self, op: u32) -> usize {
        self.boundary()
            .async_facilities
            .iter()
            .position(|c| c.op_type == op)
            .expect("the boundary has the async facility of every async export")
    }

    /// The dispatchers of the async facilities: each asks a handler for an operation's answer.
    fn js_perform(&self, w: &mut Writer) {
        let b = self.boundary();
        for (c, n) in b.async_facilities.iter().zip(&self.names.asyncs) {
            let decl = &b.table.types[c.op_type as usize];
            let tn = &self.names.types[c.op_type as usize];
            w.line("");
            w.line(format!("/** Asks `handler` for the answer to `op`, an operation of {}. */", c.id));
            w.line(format!("function {}(handler, op) {{", n.perform));
            let call = |k: usize, x: &str| {
                let args: Vec<String> = tn.ctors[k].1.iter().map(|f| format!("{x}.{f}")).collect();
                format!(
                    "return [{}, handler.{}({})];",
                    self.descriptor(&c.operations[k].answer, Scoped::Function),
                    n.methods[k],
                    args.join(", ")
                )
            };
            if decl.ctors.len() == 1 {
                w.line(format!("  {}", call(0, "op")));
            } else {
                w.line("  switch (op.kind) {");
                for (k, (kind, _)) in tn.ctors.iter().enumerate() {
                    w.line(format!("    case {}: {}", js_string(kind), call(k, "op")));
                }
                w.line("  }");
                w.line(format!(
                    "  throw new L.MalformedError(`an operation that is not a {}: ${{op?.kind}}`);",
                    tn.name
                ));
            }
            w.line("}");
        }
    }

    fn javascript(&self) -> String {
        let b = self.boundary();
        let mut w = Writer::new();
        w.line(self.banner());
        w.line("import * as L from \"lungo-ts\";");
        for (module, alias) in self.aliases {
            w.line(format!("import * as {alias} from {};", js_string(module)));
        }
        for (&i, ext) in self.externs {
            let lean = &b.types[i].lean_name;
            w.line(format!(
                "if ({}.leanTypes[{}]?.fingerprint !== {}) {{",
                self.aliases[&ext.package],
                js_string(lean),
                js_string(&b.types[i].fingerprint)
            ));
            w.line(format!(
                "  throw new Error({});",
                js_string(&format!(
                    "{lean} of {} has another layout than the one this program was generated for: regenerate both from the same Lean definition",
                    ext.package
                ))
            ));
            w.line("}");
        }
        w.line("");
        w.line("async function programModule() {");
        w.line("  const url = new URL(\"./program.wasm\", import.meta.url);");
        w.line("  if (typeof process !== \"undefined\" && process.versions?.node) {");
        w.line("    const { readFile } = await import(\"node:fs/promises\");");
        w.line("    return readFile(url);");
        w.line("  }");
        w.line("  const response = await fetch(url);");
        w.line("  if (!response.ok) throw new Error(`cannot load ${url}: ${response.status}`);");
        w.line("  return response.arrayBuffer();");
        w.line("}");
        for i in 0..b.types.len() {
            if self.externs.contains_key(&i) {
                self.js_extern_type(&mut w, i);
            } else if b.types[i].opaque {
                self.js_opaque_type(&mut w, i);
            } else {
                self.js_type(&mut w, i);
            }
        }
        self.js_perform(&mut w);
        w.line("");
        w.line("/** The program's assurance document: what its Lean code claims and proves of each export, what it");
        w.line(" *  trusts, and what it assumes of the host's facilities. */");
        w.line(format!("export const ASSURANCE = L.parseAssurance({});", self.request.assurance.to_json().trim_end()));
        w.line("");
        w.line("/** The layout fingerprint and descriptor of each type, by Lean name, for modules using them. */");
        w.line("export const leanTypes = Object.freeze({");
        for (i, t) in b.types.iter().enumerate().filter(|(i, _)| !self.externs.contains_key(i)) {
            w.line(format!(
                "  {}: Object.freeze({{ fingerprint: {}, type: {} }}),",
                js_string(&t.lean_name),
                js_string(&t.fingerprint),
                self.names.types[i].descriptor
            ));
        }
        w.line("});");
        w.line("");
        w.line(format!("/** The Lean program {}. */", self.request.program.name));
        w.line(format!("export class {} {{", self.names.class));
        w.line("  #program;");
        w.line("  constructor(program) {");
        w.line("    this.#program = program;");
        w.line("  }");
        for (f, name) in b.functions.iter().zip(&self.names.functions) {
            self.js_function(&mut w, f, name);
        }
        if let Some(run_main) = &b.run_main {
            w.line("  runMain(args) {");
            w.line(format!("    return this.#program.runMain({}, args);", js_string(run_main)));
            w.line("  }");
        }
        if !b.async_facilities.is_empty() {
            w.line("  outstanding() {");
            w.line("    return this.#program.outstanding();");
            w.line("  }");
        }
        w.line("}");
        w.line("");
        w.line("/** Instantiates the program; `options.facilities` provides the facilities it needs. */");
        w.line("export async function load(options = {}) {");
        if !b.facilities.is_empty() {
            // Every facility, with every operation, before anything is instantiated.
            w.line("  const facilities = options.facilities ?? {};");
            for (c, n) in b.facilities.iter().zip(&self.names.facilities) {
                for (o, m) in c.operations.iter().zip(&n.methods) {
                    w.line(format!(
                        "  if (typeof facilities.{}?.{m} !== \"function\") throw new L.MissingFacilityError({}, {});",
                        n.key,
                        js_string(&c.id),
                        js_string(operation_name(&o.declaration))
                    ));
                }
            }
        }
        w.line("  const program = await L.Program.load(options.module ?? (await programModule()), { wasi: options.wasi });");
        w.line(format!("  program.types({});", js_string(&b.types_symbol)));
        for (c, n) in b.facilities.iter().zip(&self.names.facilities) {
            w.line(format!("  const {} = facilities.{};", n.key, n.key));
            for (o, m) in c.operations.iter().zip(&n.methods) {
                let ps: Vec<String> = o.params.iter().map(|p| self.descriptor(&p.ty, Scoped::Function)).collect();
                w.line(format!(
                    "  program.hostExtern({}, {}, [{}], {}, (...args) => {}.{m}(...args));",
                    js_string(&b.set_host_extern),
                    o.index,
                    ps.join(", "),
                    self.returns(&o.returns),
                    n.key
                ));
            }
        }
        w.line(format!("  return new {}(program);", self.names.class));
        w.line("}");
        w.finish()
    }

    /// A type another module provides: its values are that module's, described here by this
    /// program's own table index.
    fn js_extern_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let n = b.table.types[i].params;
        let tn = &self.names.types[i];
        let ext = self.externs[&i];
        let params: Vec<String> = (0..n).map(|k| param_name(k).to_lowercase()).collect();
        let exprs: Vec<String> = params.iter().map(|p| format!(", {p}.expr")).collect();
        w.line("");
        w.line(format!("/** Describes {}, provided by {}. */", b.types[i].lean_name, ext.package));
        w.line(format!("function {}({}) {{", tn.descriptor, params.join(", ")));
        w.line(format!(
            "  const provided = {}.leanTypes[{}].type({});",
            self.aliases[&ext.package],
            js_string(&b.types[i].lean_name),
            params.join(", ")
        ));
        w.line(format!(
            "  return new L.Type(L.inductiveExpr({i}{}), (w, v) => provided.encode(w, v), (r) => provided.decode(r), provided.canBeNull);",
            exprs.join("")
        ));
        w.line("}");
    }

    /// A type whose values cross as handles: a class of its own around `L.Opaque`.
    fn js_opaque_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let tn = &self.names.types[i];
        w.line("");
        w.line(format!(
            "/** Lean's {}, held by handle: only the program's functions make and read its values. */",
            named.lean_name
        ));
        w.line(format!("export class {} extends L.Opaque {{}}", tn.name));
        w.line("");
        w.line(format!("/** Describes {} for polymorphic functions. */", named.lean_name));
        w.line(format!("export function {}() {{", tn.descriptor));
        w.line("  return new L.Type(");
        w.line(format!("    L.inductiveExpr({i}),"));
        w.line("    (w, v) => {");
        w.line(format!(
            "      if (!(v instanceof {})) throw new L.MalformedError(\"not a {}\");",
            tn.name, named.lean_name
        ));
        w.line("      L.OPAQUE.encode(w, v);");
        w.line("    },");
        w.line(format!("    (r) => new {}(r.program, r.u64()),", tn.name));
        w.line("  );");
        w.line("}");
    }

    fn js_type(&self, w: &mut Writer, i: usize) {
        let b = self.boundary();
        let named = &b.types[i];
        let decl = &b.table.types[i];
        let tn = &self.names.types[i];
        let n = decl.params;
        let params: Vec<String> = (0..n).map(|k| param_name(k).to_lowercase()).collect();
        let exprs: Vec<String> = params.iter().map(|p| format!(", {p}.expr")).collect();
        w.line("");
        w.line(format!("/** Describes {} for polymorphic functions. */", named.lean_name));
        w.line(format!("export function {}({}) {{", tn.descriptor, params.join(", ")));
        w.line("  return new L.Type(");
        w.line(format!("    L.inductiveExpr({i}{}),", exprs.join("")));
        w.line("    (w, v) => {");
        w.line(format!(
            "      if (typeof v !== \"object\" || v === null) throw new L.MalformedError(\"not a {}\");",
            named.lean_name
        ));
        let trivial = decl.trivial.is_some();
        if decl.ctors.len() == 1 {
            if !trivial {
                w.line("      w.u32(0);");
            }
            for (f, fname) in decl.ctors[0].fields.iter().zip(&tn.ctors[0].1) {
                w.line(format!("      {}.encode(w, v.{fname});", self.descriptor(&f.ty, Scoped::Descriptor)));
            }
        } else {
            w.line("      switch (v.kind) {");
            for (ci, (c, (kind, fields))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                w.line(format!("        case {}:", js_string(kind)));
                w.line(format!("          w.u32({ci});"));
                for (f, fname) in c.fields.iter().zip(fields) {
                    w.line(format!("          {}.encode(w, v.{fname});", self.descriptor(&f.ty, Scoped::Descriptor)));
                }
                w.line("          return;");
            }
            w.line("        default:");
            w.line(format!(
                "          throw new L.MalformedError(`${{String(v.kind)}} is not a constructor of {}`);",
                named.lean_name
            ));
            w.line("      }");
        }
        w.line("    },");
        w.line("    (r) => {");
        let object = |c: &lungo_runtime::wire::Ctor, kind: Option<&str>, fields: &[String]| -> String {
            let mut parts: Vec<String> = kind.map(|k| format!("kind: {}", js_string(k))).into_iter().collect();
            parts.extend(
                c.fields
                    .iter()
                    .zip(fields)
                    .map(|(f, fname)| format!("{fname}: {}.decode(r)", self.descriptor(&f.ty, Scoped::Descriptor))),
            );
            format!("({{ {} }})", parts.join(", "))
        };
        if decl.ctors.len() == 1 {
            if !trivial {
                w.line("      const c = r.u32();");
                w.line(format!(
                    "      if (c !== 0) throw new L.MalformedError(`constructor index ${{c}} of {}`);",
                    named.lean_name
                ));
            }
            w.line(format!("      return {};", object(&decl.ctors[0], None, &tn.ctors[0].1)));
        } else {
            w.line("      const c = r.u32();");
            w.line("      switch (c) {");
            for (ci, (c, (kind, fields))) in decl.ctors.iter().zip(&tn.ctors).enumerate() {
                w.line(format!("        case {ci}: return {};", object(c, Some(kind), fields)));
            }
            w.line(format!(
                "        default: throw new L.MalformedError(`constructor index ${{c}} of {}`);",
                named.lean_name
            ));
            w.line("      }");
        }
        w.line("    },");
        w.line("  );");
        w.line("}");
    }

    fn js_function(&self, w: &mut Writer, f: &Function, name: &str) {
        let n = f.type_params.len() as u32;
        let locals = distinct_locals(f.params.iter().enumerate().map(|(i, p)| js_local(&p.name, i)).collect());
        let mut params: Vec<String> = (0..n).map(|k| format!("type{}", param_name(k))).collect();
        params.extend(locals.iter().cloned());
        let type_args: Vec<String> = (0..n).map(|k| format!("type{}", param_name(k))).collect();
        let args: Vec<String> = f
            .params
            .iter()
            .zip(&locals)
            .map(|(p, l)| format!("[{}, {l}]", self.descriptor(&p.ty, Scoped::Function)))
            .collect();
        if let Returns::Async { op, .. } = &f.returns {
            let an = &self.names.asyncs[self.async_index(*op)];
            let mut ps = vec!["handler".to_owned()];
            ps.extend(params);
            ps.push("options = {}".to_owned());
            w.line(format!("  {name}({}) {{", ps.join(", ")));
            w.line(format!(
                "    return this.#program.driveAsync({}, [{}], [{}], {}, (op) => {}(handler, op), options.signal);",
                js_string(&f.symbol),
                type_args.join(", "),
                args.join(", "),
                self.returns(&f.returns),
                an.perform
            ));
            w.line("  }");
            return;
        }
        let unit = matches!(
            &f.returns,
            Returns::Value(Type::Unit) | Returns::Io(Type::Unit) | Returns::Eio { value: Type::Unit, .. }
        );
        w.line(format!("  {name}({}) {{", params.join(", ")));
        w.line(format!(
            "    {}this.#program.invoke({}, [{}], [{}], {});",
            if unit { "" } else { "return " },
            js_string(&f.symbol),
            type_args.join(", "),
            args.join(", "),
            self.returns(&f.returns)
        ));
        w.line("  }");
    }

    fn declarations(&self) -> String {
        let b = self.boundary();
        let mut w = Writer::new();
        w.line(self.banner());
        w.line("import * as L from \"lungo-ts\";");
        for (module, alias) in self.aliases {
            w.line(format!("import * as {alias} from {};", js_string(module)));
        }
        for i in 0..b.types.len() {
            let named = &b.types[i];
            let decl = &b.table.types[i];
            let tn = &self.names.types[i];
            let n = decl.params;
            if self.externs.contains_key(&i) {
                continue;
            }
            if named.opaque {
                w.line("");
                w.line(format!(
                    "/** Lean's {}, held by handle: only the program's functions make and read its values. */",
                    named.lean_name
                ));
                w.line(format!("export class {} extends L.Opaque {{", tn.name));
                w.line("  private constructor();");
                w.line("}");
                w.line(format!("export function {}(): L.Type<{}>;", tn.descriptor, tn.name));
                continue;
            }
            let generics = if n == 0 {
                String::new()
            } else {
                format!("<{}>", (0..n).map(param_name).collect::<Vec<_>>().join(", "))
            };
            w.line("");
            w.line(format!("/** Lean's {}. */", named.lean_name));
            if decl.ctors.len() == 1 {
                w.line(format!("export interface {}{generics} {{", tn.name));
                for (f, fname) in decl.ctors[0].fields.iter().zip(&tn.ctors[0].1) {
                    w.line(format!("  readonly {fname}: {};", self.ts_type(&f.ty)));
                }
                w.line("}");
            } else {
                w.line(format!("export type {}{generics} =", tn.name));
                for (c, (kind, fields)) in decl.ctors.iter().zip(&tn.ctors) {
                    let mut parts = vec![format!("readonly kind: {}", js_string(kind))];
                    parts.extend(
                        c.fields
                            .iter()
                            .zip(fields)
                            .map(|(f, fname)| format!("readonly {fname}: {}", self.ts_type(&f.ty))),
                    );
                    w.line(format!("  | {{ {} }}", parts.join("; ")));
                }
                w.line(";");
            }
            let params: Vec<String> =
                (0..n).map(|k| format!("{}: L.Type<{}>", param_name(k).to_lowercase(), param_name(k))).collect();
            w.line(format!(
                "export function {}{generics}({}): L.Type<{}{generics}>;",
                tn.descriptor,
                params.join(", "),
                tn.name
            ));
        }
        w.line("");
        w.line("/** The layout fingerprint and descriptor of each type, by Lean name, for modules using them. */");
        w.line("export const leanTypes: {");
        w.line("  readonly [lean: string]: { readonly fingerprint: string; readonly type: (...args: L.Type<any>[]) => L.Type<any> };");
        w.line("};");
        w.line("");
        w.line(format!("/** The Lean program {}. */", self.request.program.name));
        w.line(format!("export class {} {{", self.names.class));
        w.line("  private constructor();");
        for (f, name) in b.functions.iter().zip(&self.names.functions) {
            let n = f.type_params.len() as u32;
            let generics = if n == 0 {
                String::new()
            } else {
                format!("<{}>", (0..n).map(param_name).collect::<Vec<_>>().join(", "))
            };
            let locals = distinct_locals(f.params.iter().enumerate().map(|(i, p)| js_local(&p.name, i)).collect());
            let mut params: Vec<String> =
                (0..n).map(|k| format!("type{}: L.Type<{}>", param_name(k), param_name(k))).collect();
            params.extend(f.params.iter().zip(&locals).map(|(p, l)| format!("{l}: {}", self.ts_type(&p.ty))));
            if let Returns::Async { op, .. } = &f.returns {
                params.insert(0, format!("handler: {}", self.names.asyncs[self.async_index(*op)].handler));
                params.push("options?: { signal?: AbortSignal }".to_owned());
            }
            w.line(format!(
                "  /** Lean's {} : {} */",
                f.lean_name,
                f.lean_type.replace('\n', " ").replace("*/", "* /")
            ));
            w.line(format!("  {name}{generics}({}): {};", params.join(", "), self.result_type(&f.returns)));
        }
        if b.run_main.is_some() {
            w.line("  /** Runs the Lean program's `main` with `args`; its exit code. */");
            w.line("  runMain(args: string[]): number;");
        }
        if !b.async_facilities.is_empty() {
            w.line(
                "  /** The number of this program's async calls waiting for an answer: zero once each has settled. */",
            );
            w.line("  outstanding(): number;");
        }
        w.line("}");
        let facilities = format!("{}Facilities", self.names.class);
        for (c, n) in b.facilities.iter().zip(&self.names.facilities) {
            w.line("");
            let note = crate::c::api::assumptions_note(self.request, &c.lean_name);
            w.line(format!(
                "/** The facility {} (`{}`), which the host provides; a method whose Lean type is not IO or EIO must not throw.{} */",
                c.id,
                c.lean_name,
                note.replace("*/", "* /")
            ));
            w.line(format!("export interface {} {{", n.interface));
            for (o, m) in c.operations.iter().zip(&n.methods) {
                let locals = distinct_locals(o.params.iter().enumerate().map(|(i, p)| js_local(&p.name, i)).collect());
                let params: Vec<String> =
                    o.params.iter().zip(&locals).map(|(p, l)| format!("{l}: {}", self.ts_type(&p.ty))).collect();
                w.line(format!(
                    "  /** Lean's {} : {} */",
                    o.declaration,
                    o.lean_type.as_deref().unwrap_or("?").replace('\n', " ").replace("*/", "* /")
                ));
                w.line(format!("  {m}({}): {};", params.join(", "), self.result_type(&o.returns)));
            }
            w.line("}");
        }
        if !b.facilities.is_empty() {
            w.line("");
            w.line("/** The facilities the program needs, by name. */");
            w.line(format!("export interface {facilities} {{"));
            for n in &self.names.facilities {
                w.line(format!("  {}: {};", n.key, n.interface));
            }
            w.line("}");
        }
        for (c, n) in b.async_facilities.iter().zip(&self.names.asyncs) {
            let decl = &b.table.types[c.op_type as usize];
            let tn = &self.names.types[c.op_type as usize];
            w.line("");
            let note = crate::c::api::assumptions_note(self.request, &c.lean_name);
            w.line(format!(
                "/** Performs the operations of the async facility {} (`{}`) an async export asks; a rejection abandons the program.{} */",
                c.id,
                c.lean_name,
                note.replace("*/", "* /")
            ));
            w.line(format!("export interface {} {{", n.handler));
            for ((o, ctor), m) in c.operations.iter().zip(&decl.ctors).zip(&n.methods) {
                let fields = &tn.ctors[o.ctor as usize].1;
                let params: Vec<String> =
                    ctor.fields.iter().zip(fields).map(|(f, l)| format!("{l}: {}", self.ts_type(&f.ty))).collect();
                let a = self.ts_type(&o.answer);
                w.line(format!("  /** Performs {}. */", o.lean_name));
                w.line(format!("  {m}({}): {a} | Promise<{a}>;", params.join(", ")));
            }
            w.line("}");
        }
        w.line("");
        w.line("/** The program's assurance document. */");
        w.line("export const ASSURANCE: L.Assurance;");
        w.line("");
        w.line("export interface LoadOptions {");
        if !b.facilities.is_empty() {
            w.line(format!("  facilities: {facilities};"));
        }
        w.line("  /** The program module (default: `program.wasm` next to this file). */");
        w.line("  module?: WebAssembly.Module | BufferSource;");
        w.line("  wasi?: L.Wasi;");
        w.line("}");
        w.line("");
        let optional = if b.facilities.is_empty() { "?" } else { "" };
        w.line(format!("export function load(options{optional}: LoadOptions): Promise<{}>;", self.names.class));
        w.finish()
    }
}
