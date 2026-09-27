//! The TypeScript binding: an npm package (ES module with type declarations) whose program is a
//! WebAssembly module (`program.wasm`, which the `lungo` command links), running on the support
//! library `lungo-ts`.
//!
//! Structures are plain objects, inductive types of several constructors objects tagged with
//! `kind`, `Nat`, `Int` and 64-bit integers `bigint`, smaller numbers `number`. `load()`
//! instantiates the module and returns an object whose methods are the program's functions; they
//! throw `LeanIOError` (`IO`), `LeanError` (`EIO ε`) and `MalformedError` (arguments Lean cannot
//! represent).

use crate::CodegenError;
use crate::c::boundary::{Boundary, Function};
use crate::core::names::{components, lower_camel_case, upper_camel_case};
use crate::core::naming::{Scope, distinct_locals, short_names};
use crate::core::writer::Writer;
use crate::plugin::{Distribution, GenerateRequest, Generator, options};
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
    if id.starts_with("type") || ["w", "r", "v", "options", "host"].contains(&id.as_str()) {
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
    descriptor: String,
    /// Per constructor: its `kind` and fields.
    ctors: Vec<(String, Vec<String>)>,
}

struct Names {
    class: String,
    types: Vec<TypeNames>,
    functions: Vec<String>,
    host_methods: Vec<String>,
}

impl Names {
    fn new(b: &Boundary, class: &str) -> Result<Names, CodegenError> {
        let mut scope = Scope::new("TypeScript");
        for reserved in [class.to_owned(), format!("{class}Host"), "load".into(), "LoadOptions".into()] {
            scope.claim(reserved, "a generated declaration")?;
        }
        let type_names: Vec<&str> = b.types.iter().map(|t| t.lean_name.as_str()).collect();
        let mut types = Vec::new();
        for ((named, decl), short) in b.types.iter().zip(&b.table.types).zip(short_names(&type_names)) {
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
        let fn_names: Vec<&str> = b.functions.iter().map(|f| f.lean_name.as_str()).collect();
        let functions = short_names(&fn_names)
            .iter()
            .zip(&b.functions)
            .map(|(s, f)| methods.claim(camel(s).trim_end_matches('_').to_owned(), format!("function {}", f.lean_name)))
            .collect::<Result<_, _>>()?;
        let host_names: Vec<&str> = b.host_externs.iter().map(|h| h.declaration.as_str()).collect();
        let mut host = Scope::new("TypeScript");
        let host_methods = short_names(&host_names)
            .iter()
            .zip(&b.host_externs)
            .map(|(s, h)| {
                host.claim(camel(s).trim_end_matches('_').to_owned(), format!("host extern {}", h.declaration))
            })
            .collect::<Result<_, _>>()?;
        Ok(Names { class: class.to_owned(), types, functions, host_methods })
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
        let opts = options(request, "ts", &["package", "version"]).map_err(|e| vec![e])?;
        let b = &request.boundary;
        let package = opts.get("package").map(|s| s.to_string()).unwrap_or_else(|| request.program.name.to_lowercase());
        if !valid_npm_name(&package) {
            return Err(vec![CodegenError::Configuration(format!(
                "`{package}` cannot name an npm package (option `package`)"
            ))]);
        }
        let version = opts.get("version").copied().unwrap_or("0.1.0");
        let class = upper_camel_case(&request.program.name);
        let names = Names::new(b, &class).map_err(|e| vec![e])?;
        let e = Emitter { request, names: &names };
        let mut files = BTreeMap::new();
        files.insert("index.js".to_owned(), e.javascript());
        files.insert("index.d.ts".to_owned(), e.declarations());
        files.insert("package.json".to_owned(), package_json(request, &package, version));
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
        "files": ["index.js", "index.d.ts", "program.wasm"],
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
        }
    }

    fn javascript(&self) -> String {
        let b = self.boundary();
        let mut w = Writer::new();
        w.line(self.banner());
        w.line("import * as L from \"lungo-ts\";");
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
            self.js_type(&mut w, i);
        }
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
        w.line("}");
        w.line("");
        w.line("/** Instantiates the program; `options.host` implements its externs. */");
        w.line("export async function load(options = {}) {");
        w.line("  const program = await L.Program.load(options.module ?? (await programModule()), { wasi: options.wasi });");
        w.line(format!("  program.types({});", js_string(&b.types_symbol)));
        if !b.host_externs.is_empty() {
            w.line("  const host = options.host;");
            let methods: Vec<String> = self.names.host_methods.iter().map(|m| format!("`{m}`")).collect();
            w.line(format!(
                "  if (!host) throw new TypeError(\"load() needs options.host implementing {}\");",
                methods.join(", ").replace('"', "\\\"")
            ));
            for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
                let ps: Vec<String> = h.params.iter().map(|p| self.descriptor(&p.ty, Scoped::Function)).collect();
                w.line(format!(
                    "  program.hostExtern({}, {}, [{}], {}, (...args) => host.{m}(...args));",
                    js_string(&b.set_host_extern),
                    h.index,
                    ps.join(", "),
                    self.returns(&h.returns)
                ));
            }
        }
        w.line(format!("  return new {}(program);", self.names.class));
        w.line("}");
        w.finish()
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
        for i in 0..b.types.len() {
            let named = &b.types[i];
            let decl = &b.table.types[i];
            let tn = &self.names.types[i];
            let n = decl.params;
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
        w.line("}");
        let host = format!("{}Host", self.names.class);
        if !b.host_externs.is_empty() {
            w.line("");
            w.line(
                "/** Implements the program's externs; a method whose Lean type is not IO or EIO must not throw. */",
            );
            w.line(format!("export interface {host} {{"));
            for (h, m) in b.host_externs.iter().zip(&self.names.host_methods) {
                let locals = distinct_locals(h.params.iter().enumerate().map(|(i, p)| js_local(&p.name, i)).collect());
                let params: Vec<String> =
                    h.params.iter().zip(&locals).map(|(p, l)| format!("{l}: {}", self.ts_type(&p.ty))).collect();
                w.line(format!(
                    "  /** Lean's {} : {} */",
                    h.declaration,
                    h.lean_type.as_deref().unwrap_or("?").replace('\n', " ").replace("*/", "* /")
                ));
                w.line(format!("  {m}({}): {};", params.join(", "), self.result_type(&h.returns)));
            }
            w.line("}");
        }
        w.line("");
        w.line("export interface LoadOptions {");
        if !b.host_externs.is_empty() {
            w.line(format!("  host: {host};"));
        }
        w.line("  /** The program module (default: `program.wasm` next to this file). */");
        w.line("  module?: WebAssembly.Module | BufferSource;");
        w.line("  wasi?: L.Wasi;");
        w.line("}");
        w.line("");
        let optional = if b.host_externs.is_empty() { "?" } else { "" };
        w.line(format!("export function load(options{optional}: LoadOptions): Promise<{}>;", self.names.class));
        w.finish()
    }
}
