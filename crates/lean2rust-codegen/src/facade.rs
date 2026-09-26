//! The public facade: idiomatic Rust types and functions over the compiler layer.
//!
//! Lean declarations are placed in a Rust module tree mirroring their namespaces. Names are
//! sanitized deterministically; when sanitization makes two Lean names collide, the
//! lexicographically first keeps the plain identifier and the others receive numeric suffixes.
//! Every mapping is recorded, keyed by the canonical Lean name.
//!
//! Types with a first-order runtime representation become Rust enums and structs whose
//! conversions follow Lean's constructor layouts exactly; everything else is an opaque
//! [`LeanValue`](lean2rust::LeanValue).

use crate::compiler::rust_type;
use crate::names::{components, mangle};
use crate::rust::{self, Writer, camel, snake};
use crate::{CodegenError, NameRecord};
use lean2rust_bir::IrType;
use lean2rust_protocol::{
    CtorDecl, Export, ExternRequirement, FacadeParam, FacadeSignature, FacadeType, FieldKind, TypeDecl,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// A generated Rust item with its module path (relative to the aggregate root).
#[derive(Clone, Debug)]
pub struct Placement {
    pub module: Vec<String>,
    pub ident: String,
}

impl Placement {
    pub fn rust_path(&self) -> String {
        let mut parts = self.module.clone();
        parts.push(self.ident.clone());
        parts.join("::")
    }

    /// The path of this item as seen from a module at `depth`.
    pub fn path_from_depth(&self, depth: usize) -> String {
        format!("{}{}", "super::".repeat(depth), self.rust_path())
    }
}

#[derive(Default)]
struct ModuleNode {
    /// Identifiers in the type namespace (types and submodules).
    types: BTreeSet<String>,
    /// Identifiers in the value namespace (functions).
    values: BTreeSet<String>,
    children: BTreeMap<String, (String, ModuleNode)>,
}

/// Allocates collision-free Rust identifiers for every public item.
pub struct Naming {
    facade_namespace: String,
    root: ModuleNode,
    pub records: Vec<NameRecord>,
}

pub enum ItemKind {
    Function,
    Type,
}

impl Naming {
    pub fn new(facade_namespace: &str) -> Self {
        Naming { facade_namespace: facade_namespace.to_owned(), root: ModuleNode::default(), records: Vec::new() }
    }

    /// Namespace components of `lean_name` relative to the facade namespace.
    fn relative(&self, lean_name: &str) -> (Vec<String>, String) {
        let mut comps = components(lean_name);
        let last = comps.pop().expect("a Lean name has at least one component");
        if comps.first().map(String::as_str) == Some(self.facade_namespace.as_str()) {
            comps.remove(0);
        } else {
            comps.insert(0, "_root_".to_owned());
        }
        (comps, last)
    }

    fn module_for(&mut self, namespace: &[String]) -> Vec<String> {
        let mut node = &mut self.root;
        let mut path = Vec::new();
        for comp in namespace {
            let ident = if comp == "_root_" { "_root_".to_owned() } else { snake(comp) };
            if !node.children.contains_key(comp) {
                // A submodule shares the type namespace with the types of its parent.
                let mut unique = ident.clone();
                let mut k = 1;
                while node.types.contains(&unique) {
                    unique = format!("{ident}_ns{k}");
                    k += 1;
                }
                node.types.insert(unique.clone());
                node.children.insert(comp.clone(), (unique, ModuleNode::default()));
            }
            let (ident, child) = node.children.get_mut(comp).expect("inserted above");
            path.push(ident.clone());
            node = child;
        }
        path
    }

    /// Places `lean_name`. Names must be placed in sorted order for deterministic suffixes.
    pub fn place(&mut self, lean_name: &str, kind: ItemKind) -> Placement {
        let (namespace, last) = self.relative(lean_name);
        let module = self.module_for(&namespace);
        let mut node = &mut self.root;
        for comp in &namespace {
            node = &mut node.children.get_mut(comp).expect("module exists").1;
        }
        let (base, set, kind_name) = match kind {
            ItemKind::Function => (snake(&last), &mut node.values, "function"),
            ItemKind::Type => (camel(&last), &mut node.types, "type"),
        };
        let mut ident = base.clone();
        let mut k = 1;
        while set.contains(&ident) || ident.starts_with("__") {
            ident = format!("{}_{k}", base.trim_start_matches("r#"));
            k += 1;
        }
        set.insert(ident.clone());
        let placement = Placement { module, ident };
        self.records.push(NameRecord {
            lean_name: lean_name.to_owned(),
            kind: kind_name.to_owned(),
            rust_path: placement.rust_path(),
            renamed: ident_differs(&last, &placement.ident),
        });
        placement
    }
}

fn ident_differs(lean: &str, rust: &str) -> bool {
    rust.trim_start_matches("r#") != lean
}

/// Greek letters conventionally used for Lean type parameters.
fn type_param_ident(lean: &str, index: usize, taken: &BTreeSet<String>) -> String {
    let base = match lean {
        "α" => "A".to_owned(),
        "β" => "B".to_owned(),
        "γ" => "C".to_owned(),
        "δ" => "D".to_owned(),
        "ε" => "E".to_owned(),
        "σ" => "S".to_owned(),
        "τ" => "T".to_owned(),
        "ω" => "W".to_owned(),
        other => {
            let c = camel(other);
            if c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') && !c.starts_with('_') {
                c
            } else {
                format!("T{index}")
            }
        }
    };
    let mut ident = base.clone();
    let mut k = 1;
    while taken.contains(&ident) {
        ident = format!("{base}{k}");
        k += 1;
    }
    ident
}

pub struct Facade<'a> {
    pub types: HashMap<&'a str, &'a TypeDecl>,
    pub placements: HashMap<String, Placement>,
    /// Opaque marker types, by Lean head constant (`None` for types without one).
    pub markers: BTreeMap<Option<String>, String>,
    /// Types in the same recursion group, for boxing recursive fields.
    groups: HashMap<String, usize>,
    /// The module (relative to the aggregate root) defining the backend, or `None` for the
    /// lean2rust runtime.
    backend_module: Option<String>,
    /// Name records of constructors and fields, in emission order.
    pub members: Vec<NameRecord>,
}

impl<'a> Facade<'a> {
    pub fn new(
        types: &'a [TypeDecl],
        naming: &mut Naming,
        exports: &[Export],
        backend_module: Option<String>,
    ) -> Result<Self, CodegenError> {
        let mut placements = HashMap::new();
        let mut names: Vec<&str> = types.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        for n in &names {
            placements.insert((*n).to_owned(), naming.place(n, ItemKind::Type));
        }
        let mut exported: Vec<&str> = exports.iter().map(|e| e.name.as_str()).collect();
        exported.sort();
        for n in &exported {
            placements.insert((*n).to_owned(), naming.place(n, ItemKind::Function));
        }
        let types_map: HashMap<&str, &TypeDecl> = types.iter().map(|t| (t.name.as_str(), t)).collect();
        let groups = recursion_groups(types);
        Ok(Facade {
            types: types_map,
            placements,
            markers: BTreeMap::new(),
            groups,
            backend_module,
            members: Vec::new(),
        })
    }

    /// The path of the object backend, as seen from a module at `depth`.
    pub fn backend(&self, depth: usize) -> String {
        match &self.backend_module {
            None => "::lean2rust::RustBackend".to_owned(),
            Some(m) => format!("{}{m}::Backend", "super::".repeat(depth)),
        }
    }

    fn placement(&self, lean_name: &str) -> Result<&Placement, CodegenError> {
        self.placements
            .get(lean_name)
            .ok_or_else(|| CodegenError::internal(format!("no facade placement for {lean_name}")))
    }

    /// Records the opaque marker for `head`, returning its identifier.
    fn marker(&mut self, head: &Option<String>) -> String {
        if let Some(m) = self.markers.get(head) {
            return m.clone();
        }
        let base = match head {
            Some(h) => camel(components(h).last().expect("component")),
            None => "Anonymous".to_owned(),
        };
        let base = base.trim_start_matches("r#").to_owned();
        let mut ident = base.clone();
        let mut k = 1;
        while self.markers.values().any(|v| *v == ident) {
            ident = format!("{base}{k}");
            k += 1;
        }
        self.markers.insert(head.clone(), ident.clone());
        ident
    }

    /// The Rust type for `ft`, referenced from a module at `depth`, with type parameters named
    /// by `params`.
    pub fn rust_type(&mut self, ft: &FacadeType, depth: usize, params: &[String]) -> Result<String, CodegenError> {
        Ok(match ft {
            FacadeType::Nat => "::lean2rust::Nat".into(),
            FacadeType::Int => "::lean2rust::Int".into(),
            FacadeType::Bool => "bool".into(),
            FacadeType::Uint8 => "u8".into(),
            FacadeType::Uint16 => "u16".into(),
            FacadeType::Uint32 => "u32".into(),
            FacadeType::Uint64 => "u64".into(),
            FacadeType::Usize => "usize".into(),
            FacadeType::Int8 => "i8".into(),
            FacadeType::Int16 => "i16".into(),
            FacadeType::Int32 => "i32".into(),
            FacadeType::Int64 => "i64".into(),
            FacadeType::Isize => "isize".into(),
            FacadeType::Float => "f64".into(),
            FacadeType::Float32 => "f32".into(),
            FacadeType::Char => "char".into(),
            FacadeType::String => "::std::string::String".into(),
            FacadeType::Unit => "()".into(),
            FacadeType::ByteArray => "::lean2rust::ByteArray".into(),
            FacadeType::FloatArray => "::lean2rust::FloatArray".into(),
            FacadeType::Option(t) => format!("::core::option::Option<{}>", self.rust_type(t, depth, params)?),
            FacadeType::List(t) => format!("::lean2rust::List<{}>", self.rust_type(t, depth, params)?),
            FacadeType::Array(t) => format!("::std::vec::Vec<{}>", self.rust_type(t, depth, params)?),
            FacadeType::Prod(a, b) => {
                format!("({}, {})", self.rust_type(a, depth, params)?, self.rust_type(b, depth, params)?)
            }
            FacadeType::Except { error, value } | FacadeType::Eio { error, value } => format!(
                "::core::result::Result<{}, {}>",
                self.rust_type(value, depth, params)?,
                self.rust_type(error, depth, params)?
            ),
            FacadeType::Io(t) => format!(
                "::core::result::Result<{}, ::lean2rust::IoError<{}>>",
                self.rust_type(t, depth, params)?,
                self.backend(depth)
            ),
            FacadeType::BaseIo(t) => self.rust_type(t, depth, params)?,
            FacadeType::Function { params: ps, result } => {
                let mut items = Vec::new();
                for p in ps {
                    items.push(self.rust_type(p, depth, params)?);
                }
                format!(
                    "::lean2rust::LeanClosure<fn({}) -> {}, {}>",
                    items.join(", "),
                    self.rust_type(result, depth, params)?,
                    self.backend(depth)
                )
            }
            FacadeType::Param(i) => params
                .get(*i as usize)
                .cloned()
                .ok_or_else(|| CodegenError::internal(format!("type parameter {i} out of range")))?,
            FacadeType::Inductive { name, args } => {
                let path = self.placement(name)?.path_from_depth(depth);
                if args.is_empty() {
                    path
                } else {
                    let mut items = Vec::new();
                    for a in args {
                        items.push(self.rust_type(a, depth, params)?);
                    }
                    format!("{path}<{}>", items.join(", "))
                }
            }
            FacadeType::Opaque { head, .. } => {
                let marker = self.marker(head);
                format!(
                    "::lean2rust::LeanValue<{}__opaque::{marker}, {}>",
                    "super::".repeat(depth),
                    self.backend(depth)
                )
            }
        })
    }

    /// Emits the Rust definition and conversions of every described type.
    pub fn emit_types(&mut self, modules: &mut ModuleTree, decls: &[TypeDecl]) -> Result<(), CodegenError> {
        let mut sorted: Vec<&TypeDecl> = decls.iter().collect();
        sorted.sort_by(|a, b| a.name.cmp(&b.name));
        for t in sorted {
            let placement = self.placement(&t.name)?.clone();
            let depth = placement.module.len();
            let mut w = Writer::new();
            self.emit_type(&mut w, t, &placement, depth)?;
            modules.push(&placement.module, w.finish());
        }
        Ok(())
    }

    fn type_params(&self, t: &TypeDecl) -> Vec<String> {
        let mut taken = BTreeSet::new();
        let mut out = Vec::new();
        for (i, p) in t.params.iter().enumerate() {
            let id = type_param_ident(p, i, &taken);
            taken.insert(id.clone());
            out.push(id);
        }
        out
    }

    fn capabilities(&self, ft: &FacadeType, seen: &mut BTreeSet<String>) -> (bool, bool) {
        // (PartialEq, Eq + Hash)
        match ft {
            FacadeType::Float | FacadeType::Float32 | FacadeType::FloatArray => (true, false),
            FacadeType::Opaque { .. } | FacadeType::Function { .. } | FacadeType::Io(_) => (false, false),
            FacadeType::Option(t) | FacadeType::List(t) | FacadeType::Array(t) | FacadeType::BaseIo(t) => {
                self.capabilities(t, seen)
            }
            FacadeType::Prod(a, b)
            | FacadeType::Except { error: a, value: b }
            | FacadeType::Eio { error: a, value: b } => {
                let (p1, e1) = self.capabilities(a, seen);
                let (p2, e2) = self.capabilities(b, seen);
                (p1 && p2, e1 && e2)
            }
            FacadeType::Inductive { name, args } => {
                let mut p = true;
                let mut e = true;
                for a in args {
                    let (pa, ea) = self.capabilities(a, seen);
                    p &= pa;
                    e &= ea;
                }
                if seen.insert(name.clone())
                    && let Some(t) = self.types.get(name.as_str())
                {
                    for c in &t.ctors {
                        for f in &c.fields {
                            let (pf, ef) = self.capabilities(&f.ty, seen);
                            p &= pf;
                            e &= ef;
                        }
                    }
                }
                (p, e)
            }
            _ => (true, true),
        }
    }

    /// Whether field type `ft` of type `owner` refers back to `owner`'s recursion group without
    /// an intervening heap container.
    fn needs_box(&self, owner: &str, ft: &FacadeType) -> bool {
        let group = self.groups.get(owner).copied();
        fn visit(f: &Facade, ft: &FacadeType, group: Option<usize>) -> bool {
            match ft {
                FacadeType::Inductive { name, args } => {
                    group.is_some() && f.groups.get(name.as_str()).copied() == group
                        || args.iter().any(|a| visit(f, a, group))
                }
                FacadeType::Option(t) | FacadeType::BaseIo(t) | FacadeType::Io(t) => visit(f, t, group),
                FacadeType::Prod(a, b)
                | FacadeType::Except { error: a, value: b }
                | FacadeType::Eio { error: a, value: b } => visit(f, a, group) || visit(f, b, group),
                // Vectors and closures are already indirections.
                _ => false,
            }
        }
        visit(self, ft, group)
    }

    fn emit_type(
        &mut self,
        w: &mut Writer,
        t: &TypeDecl,
        placement: &Placement,
        depth: usize,
    ) -> Result<(), CodegenError> {
        let params = self.type_params(t);
        let generics = if params.is_empty() { String::new() } else { format!("<{}>", params.join(", ")) };
        let b = self.backend(depth);
        let bounded = if params.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                params.iter().map(|p| format!("{p}: ::lean2rust::LeanType<{b}>")).collect::<Vec<_>>().join(", ")
            )
        };
        let mut seen = BTreeSet::new();
        seen.insert(t.name.clone());
        let mut partial_eq = true;
        let mut eq = true;
        for c in &t.ctors {
            for f in &c.fields {
                let (p, e) = self.capabilities(&f.ty, &mut seen);
                partial_eq &= p;
                eq &= e;
            }
        }
        let mut derives = vec!["Clone", "Debug"];
        if partial_eq {
            derives.push("PartialEq");
        }
        if eq {
            derives.extend(["Eq", "Hash"]);
        }
        let ident = &placement.ident;
        w.line(format!("/// Lean: `{}`", t.name));
        w.line(format!("#[derive({})]", derives.join(", ")));
        let is_enum = t.ctors.iter().all(|c| c.fields.is_empty()) && t.ctors.len() > 1;
        // Field and variant naming.
        let variant_idents =
            unique_idents(t.ctors.iter().map(|c| camel(components(&c.name).last().expect("ctor name"))));
        let single_struct = t.ctors.len() == 1;
        self.record_members(t, placement, &variant_idents, single_struct);
        if single_struct {
            let c = &t.ctors[0];
            let fields = self.field_decls(t, c, depth, &params)?;
            if c.fields.is_empty() {
                w.line(format!("pub struct {ident}{generics};"));
            } else if let Some(named) = named_fields(c, t.structure) {
                w.open(format!("pub struct {ident}{generics} {{"));
                for ((name, _), ty) in named.iter().zip(&fields) {
                    w.line(format!("pub {name}: {ty},"));
                }
                w.close("}");
            } else {
                w.line(format!(
                    "pub struct {ident}{generics}({});",
                    fields.iter().map(|f| format!("pub {f}")).collect::<Vec<_>>().join(", ")
                ));
            }
        } else {
            w.open(format!("pub enum {ident}{generics} {{"));
            for (c, v) in t.ctors.iter().zip(&variant_idents) {
                let fields = self.field_decls(t, c, depth, &params)?;
                if c.fields.is_empty() {
                    w.line(format!("{v},"));
                } else if let Some(named) = named_fields(c, t.structure) {
                    let items: Vec<String> =
                        named.iter().zip(&fields).map(|((n, _), ty)| format!("{n}: {ty}")).collect();
                    w.line(format!("{v} {{ {} }},", items.join(", ")));
                } else {
                    w.line(format!("{v}({}),", fields.join(", ")));
                }
            }
            w.close("}");
        }
        w.line("");
        // Conversions.
        w.line("#[allow(unused_imports, unused_unsafe)]");
        w.open(format!("unsafe impl{bounded} ::lean2rust::LeanType<{b}> for {ident}{generics} {{"));
        w.open("fn into_lean(self) -> ::lean2rust::__runtime::Obj {");
        w.line("use ::lean2rust::__runtime as rt;");
        w.open("unsafe {");
        if let Some(triv) = &t.trivial {
            let c = t
                .ctors
                .iter()
                .find(|c| c.name == triv.ctor)
                .ok_or_else(|| CodegenError::internal(format!("trivial structure ctor of {} missing", t.name)))?;
            let binders = self.binders(t, c, single_struct, &variant_idents[0], ident);
            w.line(format!("let {} = self;", binders.pattern));
            let field = &binders.names[triv.field as usize];
            let value = self.boxed_field_value(t, &c.fields[triv.field as usize].ty, field);
            w.line(format!("::lean2rust::LeanType::<{b}>::into_lean({value})"));
        } else if is_enum {
            w.open("match self {");
            for (c, v) in t.ctors.iter().zip(&variant_idents) {
                w.line(format!("{ident}::{v} => rt::lean_box({}),", c.tag));
            }
            w.close("}");
        } else if single_struct {
            let c = &t.ctors[0];
            let binders = self.binders(t, c, true, &variant_idents[0], ident);
            w.line(format!("let {} = self;", binders.pattern));
            self.emit_into_ctor(w, t, c, &binders.names, &b)?;
        } else {
            w.open("match self {");
            for (c, v) in t.ctors.iter().zip(&variant_idents) {
                let binders = self.binders(t, c, false, v, ident);
                w.open(format!("{} => {{", binders.pattern));
                self.emit_into_ctor(w, t, c, &binders.names, &b)?;
                w.close("}");
            }
            w.close("}");
        }
        w.close("}");
        w.close("}");
        w.open("unsafe fn from_lean(o: ::lean2rust::__runtime::Obj) -> Self {");
        w.line("use ::lean2rust::__runtime as rt;");
        w.open("unsafe {");
        if let Some(triv) = &t.trivial {
            let c = t.ctors.iter().find(|c| c.name == triv.ctor).expect("checked above");
            let fty = self.rust_type(&c.fields[triv.field as usize].ty, depth, &params)?;
            let inner = format!("<{fty} as ::lean2rust::LeanType<{b}>>::from_lean(o)");
            let value = self.wrap_field(t, &c.fields[triv.field as usize].ty, &inner);
            let construct = self.construct(t, c, single_struct, &variant_idents[0], ident, &[value]);
            w.line(construct);
        } else if is_enum {
            w.open("match rt::lean_obj_tag(o) {");
            for (c, v) in t.ctors.iter().zip(&variant_idents) {
                w.line(format!("{} => {ident}::{v},", c.tag));
            }
            w.line(format!(
                "tag => rt::lean_internal_panic(&::std::format!(\"invalid constructor tag {{tag}} for {}\")),",
                t.name
            ));
            w.close("}");
        } else {
            w.open("match rt::lean_obj_tag(o) {");
            for (c, v) in t.ctors.iter().zip(&variant_idents) {
                let mut values = Vec::new();
                for f in &c.fields {
                    let fty = self.rust_type(&f.ty, depth, &params)?;
                    let read = match f.kind {
                        FieldKind::Object(i) => {
                            format!("<{fty} as ::lean2rust::LeanType<{b}>>::from_lean(rt::lean_ctor_get(o, {i}))")
                        }
                        FieldKind::Usize(i) => {
                            format!("::lean2rust::__facade::from_usize::<{b}, {fty}>(rt::lean_ctor_get_usize(o, {i}))")
                        }
                        FieldKind::Scalar { offset, ty, .. } => {
                            let (getter, from) = scalar_get(ty)?;
                            format!(
                                "::lean2rust::__facade::{from}::<{b}, {fty}>(rt::{getter}(o, {}){})",
                                rust::word_offset(c.size + c.usize, offset),
                                if matches!(ty, IrType::Uint8 | IrType::Uint16) { " as usize" } else { "" }
                            )
                        }
                        FieldKind::Erased | FieldKind::Void => {
                            return Err(CodegenError::internal(format!(
                                "type {} has a field without a runtime representation",
                                t.name
                            )));
                        }
                    };
                    values.push(self.wrap_field(t, &f.ty, &read));
                }
                let construct = self.construct(t, c, single_struct, v, ident, &values);
                w.line(format!("{} => {construct},", c.tag));
            }
            w.line(format!(
                "tag => rt::lean_internal_panic(&::std::format!(\"invalid constructor tag {{tag}} for {}\")),",
                t.name
            ));
            w.close("}");
        }
        w.close("}");
        w.close("}");
        w.close("}");
        w.line("");
        Ok(())
    }

    fn field_decls(
        &mut self,
        t: &TypeDecl,
        c: &CtorDecl,
        depth: usize,
        params: &[String],
    ) -> Result<Vec<String>, CodegenError> {
        let mut out = Vec::new();
        for f in &c.fields {
            let ty = self.rust_type(&f.ty, depth, params)?;
            if self.needs_box(&t.name, &f.ty) {
                out.push(format!("::std::boxed::Box<{ty}>"));
            } else {
                out.push(ty);
            }
        }
        Ok(out)
    }

    fn boxed_field_value(&self, t: &TypeDecl, ft: &FacadeType, binder: &str) -> String {
        if self.needs_box(&t.name, ft) { format!("*{binder}") } else { binder.to_owned() }
    }

    fn wrap_field(&self, t: &TypeDecl, ft: &FacadeType, value: &str) -> String {
        if self.needs_box(&t.name, ft) { format!("::std::boxed::Box::new({value})") } else { value.to_owned() }
    }

    /// Emits the construction of the Lean object for constructor `c` from the Rust values
    /// bound to `names`, ending with the object as the block's value.
    fn emit_into_ctor(
        &self,
        w: &mut Writer,
        t: &TypeDecl,
        c: &CtorDecl,
        names: &[String],
        b: &str,
    ) -> Result<(), CodegenError> {
        if c.size == 0 && c.usize == 0 && c.ssize == 0 {
            w.line(format!("rt::lean_box({})", c.tag));
            return Ok(());
        }
        w.line(format!(
            "let o = ::lean2rust::__facade::alloc_ctor::<{b}>({}, {}, {});",
            c.tag,
            c.size,
            rust::word_offset(c.usize, c.ssize)
        ));
        for (f, name) in c.fields.iter().zip(names) {
            let value = self.boxed_field_value(t, &f.ty, name);
            match f.kind {
                FieldKind::Object(i) => {
                    w.line(format!("rt::lean_ctor_set(o, {i}, ::lean2rust::LeanType::<{b}>::into_lean({value}));"))
                }
                FieldKind::Usize(i) => w.line(format!(
                    "rt::lean_ctor_set_usize(o, {i}, ::lean2rust::__facade::to_usize::<{b}, _>({value}));"
                )),
                FieldKind::Scalar { offset, ty, .. } => {
                    let (setter, conv) = scalar_set(ty, b)?;
                    w.line(format!(
                        "rt::{setter}(o, {}, {conv});",
                        rust::word_offset(c.size + c.usize, offset),
                        conv = conv.replace("{}", &value)
                    ));
                }
                FieldKind::Erased | FieldKind::Void => {
                    return Err(CodegenError::internal(format!(
                        "type {} has a field without a runtime representation",
                        t.name
                    )));
                }
            }
        }
        w.line("o");
        Ok(())
    }

    /// Records how the constructors and fields of `t` are named in Rust. Fields are written
    /// `<constructor path>.<field>`, positional fields by index.
    fn record_members(&mut self, t: &TypeDecl, placement: &Placement, variants: &[String], single_struct: bool) {
        let type_path = placement.rust_path();
        for (c, v) in t.ctors.iter().zip(variants) {
            let lean_ctor = components(&c.name).last().expect("ctor name").clone();
            let (ctor_path, ctor_ident) = if single_struct {
                (type_path.clone(), placement.ident.clone())
            } else {
                (format!("{type_path}::{v}"), v.clone())
            };
            self.members.push(NameRecord {
                lean_name: c.name.clone(),
                kind: "constructor".into(),
                rust_path: ctor_path.clone(),
                renamed: ident_differs(&lean_ctor, &ctor_ident),
            });
            let named = named_fields(c, t.structure);
            for (i, f) in c.fields.iter().enumerate() {
                // Structure fields are named by their projection; other constructor arguments by
                // the constructor and binder (or position, for unnamed binders).
                let lean_name = if t.structure {
                    format!("{}.{}", t.name, f.name)
                } else if named.is_some() {
                    format!("{}.{}", c.name, f.name)
                } else {
                    format!("{}#{i}", c.name)
                };
                let (rust_field, renamed) = match &named {
                    Some(names) => (names[i].0.clone(), ident_differs(&f.name, &names[i].0)),
                    None => (i.to_string(), true),
                };
                self.members.push(NameRecord {
                    lean_name,
                    kind: "field".into(),
                    rust_path: format!("{ctor_path}.{rust_field}"),
                    renamed,
                });
            }
        }
    }

    fn binders(&self, t: &TypeDecl, c: &CtorDecl, single: bool, variant: &str, ident: &str) -> Binders {
        let names: Vec<String> = (0..c.fields.len()).map(|i| format!("f{i}")).collect();
        let head = if single { ident.to_owned() } else { format!("{ident}::{variant}") };
        let pattern = if c.fields.is_empty() {
            head
        } else if let Some(named) = named_fields(c, t.structure) {
            let items: Vec<String> = named.iter().zip(&names).map(|((n, _), b)| format!("{n}: {b}")).collect();
            format!("{head} {{ {} }}", items.join(", "))
        } else {
            format!("{head}({})", names.join(", "))
        };
        Binders { pattern, names }
    }

    fn construct(
        &self,
        t: &TypeDecl,
        c: &CtorDecl,
        single: bool,
        variant: &str,
        ident: &str,
        values: &[String],
    ) -> String {
        let head = if single { ident.to_owned() } else { format!("{ident}::{variant}") };
        if c.fields.is_empty() {
            head
        } else if let Some(named) = named_fields(c, t.structure) {
            let items: Vec<String> = named.iter().zip(values).map(|((n, _), v)| format!("{n}: {v}")).collect();
            format!("{head} {{ {} }}", items.join(", "))
        } else {
            format!("{head}({})", values.join(", "))
        }
    }
}

struct Binders {
    pattern: String,
    names: Vec<String>,
}

/// Rust field names for a constructor: structure fields always, other constructors' fields
/// when every binder has a distinct name that Lean did not generate (unnamed constructor
/// arguments are named `a`).
fn named_fields(c: &CtorDecl, structure: bool) -> Option<Vec<(String, ())>> {
    let names: Vec<String> = c.fields.iter().map(|f| snake(&f.name)).collect();
    let distinct: BTreeSet<&String> = names.iter().collect();
    let meaningful = structure || c.fields.iter().all(|f| !f.name.is_empty() && f.name != "a");
    (distinct.len() == names.len() && meaningful).then(|| names.into_iter().map(|n| (n, ())).collect())
}

fn unique_idents(idents: impl Iterator<Item = String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for base in idents {
        let mut id = base.clone();
        let mut k = 1;
        while !seen.insert(id.clone()) {
            id = format!("{}{k}", base.trim_start_matches("r#"));
            k += 1;
        }
        out.push(id);
    }
    out
}

/// Names of the described types reachable from the exports' and application externs'
/// signatures, following constructor fields.
pub fn reachable_types<'t>(
    types: &'t [TypeDecl],
    exports: &[Export],
    externs: &[&ExternRequirement],
) -> BTreeSet<&'t str> {
    fn collect(ft: &FacadeType, out: &mut Vec<String>) {
        match ft {
            FacadeType::Inductive { name, args } => {
                out.push(name.clone());
                for a in args {
                    collect(a, out);
                }
            }
            FacadeType::Option(t)
            | FacadeType::List(t)
            | FacadeType::Array(t)
            | FacadeType::Io(t)
            | FacadeType::BaseIo(t) => collect(t, out),
            FacadeType::Prod(a, b)
            | FacadeType::Except { error: a, value: b }
            | FacadeType::Eio { error: a, value: b } => {
                collect(a, out);
                collect(b, out);
            }
            FacadeType::Function { params, result } => {
                for p in params {
                    collect(p, out);
                }
                collect(result, out);
            }
            _ => {}
        }
    }
    let by_name: HashMap<&str, &TypeDecl> = types.iter().map(|t| (t.name.as_str(), t)).collect();
    let mut work = Vec::new();
    let params = |ps: &[FacadeParam], work: &mut Vec<String>| {
        for p in ps {
            if let FacadeParam::Value { ty, .. } = p {
                collect(ty, work);
            }
        }
    };
    for e in exports {
        params(&e.params, &mut work);
        collect(&e.result, &mut work);
    }
    for r in externs {
        if let Some(sig) = &r.facade {
            params(&sig.params, &mut work);
            collect(&sig.result, &mut work);
        }
    }
    let mut seen = BTreeSet::new();
    while let Some(n) = work.pop() {
        if let Some(t) = by_name.get(n.as_str())
            && seen.insert(t.name.as_str())
        {
            for c in &t.ctors {
                for f in &c.fields {
                    collect(&f.ty, &mut work);
                }
            }
        }
    }
    seen
}

/// Groups of mutually recursive types (strongly connected components of the field-reference
/// graph), identified by index.
fn recursion_groups(types: &[TypeDecl]) -> HashMap<String, usize> {
    let index: HashMap<&str, usize> = types.iter().enumerate().map(|(i, t)| (t.name.as_str(), i)).collect();
    let mut edges: Vec<Vec<usize>> = vec![Vec::new(); types.len()];
    fn refs(ft: &FacadeType, out: &mut Vec<String>) {
        match ft {
            FacadeType::Inductive { name, args } => {
                out.push(name.clone());
                for a in args {
                    refs(a, out);
                }
            }
            FacadeType::Option(t)
            | FacadeType::List(t)
            | FacadeType::Array(t)
            | FacadeType::Io(t)
            | FacadeType::BaseIo(t) => refs(t, out),
            FacadeType::Prod(a, b)
            | FacadeType::Except { error: a, value: b }
            | FacadeType::Eio { error: a, value: b } => {
                refs(a, out);
                refs(b, out);
            }
            FacadeType::Function { params, result } => {
                for p in params {
                    refs(p, out);
                }
                refs(result, out);
            }
            _ => {}
        }
    }
    for (i, t) in types.iter().enumerate() {
        let mut out = Vec::new();
        for c in &t.ctors {
            for f in &c.fields {
                refs(&f.ty, &mut out);
            }
        }
        for n in out {
            if let Some(j) = index.get(n.as_str()) {
                edges[i].push(*j);
            }
        }
    }
    // Tarjan's algorithm.
    struct State {
        index: Vec<Option<usize>>,
        low: Vec<usize>,
        on_stack: Vec<bool>,
        stack: Vec<usize>,
        next: usize,
        comp: Vec<usize>,
        ncomp: usize,
    }
    fn strong(v: usize, edges: &[Vec<usize>], s: &mut State) {
        s.index[v] = Some(s.next);
        s.low[v] = s.next;
        s.next += 1;
        s.stack.push(v);
        s.on_stack[v] = true;
        for &w in &edges[v] {
            match s.index[w] {
                None => {
                    strong(w, edges, s);
                    s.low[v] = s.low[v].min(s.low[w]);
                }
                Some(iw) if s.on_stack[w] => s.low[v] = s.low[v].min(iw),
                _ => {}
            }
        }
        if Some(s.low[v]) == s.index[v] {
            loop {
                let w = s.stack.pop().expect("stack");
                s.on_stack[w] = false;
                s.comp[w] = s.ncomp;
                if w == v {
                    break;
                }
            }
            s.ncomp += 1;
        }
    }
    let n = types.len();
    let mut s = State {
        index: vec![None; n],
        low: vec![0; n],
        on_stack: vec![false; n],
        stack: Vec::new(),
        next: 0,
        comp: vec![0; n],
        ncomp: 0,
    };
    for v in 0..n {
        if s.index[v].is_none() {
            strong(v, &edges, &mut s);
        }
    }
    // Only groups whose members reach themselves are recursive.
    let mut out = HashMap::new();
    for (i, t) in types.iter().enumerate() {
        let recursive = edges[i].iter().any(|&j| s.comp[j] == s.comp[i]);
        let group_recursive = recursive || (0..n).any(|k| k != i && s.comp[k] == s.comp[i]);
        if group_recursive {
            out.insert(t.name.clone(), s.comp[i]);
        }
    }
    out
}

fn scalar_set(ty: IrType, b: &str) -> Result<(&'static str, String), CodegenError> {
    Ok(match ty {
        IrType::Uint8 => ("lean_ctor_set_uint8", format!("::lean2rust::__facade::to_small::<{b}, _>({{}}) as u8")),
        IrType::Uint16 => ("lean_ctor_set_uint16", format!("::lean2rust::__facade::to_small::<{b}, _>({{}}) as u16")),
        IrType::Uint32 => ("lean_ctor_set_uint32", format!("::lean2rust::__facade::to_u32::<{b}, _>({{}})")),
        IrType::Uint64 => ("lean_ctor_set_uint64", format!("::lean2rust::__facade::to_u64::<{b}, _>({{}})")),
        IrType::Float => ("lean_ctor_set_float", format!("::lean2rust::__facade::to_f64::<{b}, _>({{}})")),
        IrType::Float32 => ("lean_ctor_set_float32", format!("::lean2rust::__facade::to_f32::<{b}, _>({{}})")),
        other => return Err(CodegenError::internal(format!("scalar field of type {}", other.name()))),
    })
}

fn scalar_get(ty: IrType) -> Result<(&'static str, &'static str), CodegenError> {
    Ok(match ty {
        IrType::Uint8 => ("lean_ctor_get_uint8", "from_small"),
        IrType::Uint16 => ("lean_ctor_get_uint16", "from_small"),
        IrType::Uint32 => ("lean_ctor_get_uint32", "from_u32"),
        IrType::Uint64 => ("lean_ctor_get_uint64", "from_u64"),
        IrType::Float => ("lean_ctor_get_float", "from_f64"),
        IrType::Float32 => ("lean_ctor_get_float32", "from_f32"),
        other => return Err(CodegenError::internal(format!("scalar field of type {}", other.name()))),
    })
}

/// Converts a facade value `value` to the compiler representation `ir` (an owned object for
/// object positions).
fn to_compiler(ir: IrType, value: &str, b: &str) -> String {
    match ir {
        IrType::Uint8 => format!("::lean2rust::__facade::to_small::<{b}, _>({value}) as u8"),
        IrType::Uint16 => format!("::lean2rust::__facade::to_small::<{b}, _>({value}) as u16"),
        IrType::Uint32 => format!("::lean2rust::__facade::to_u32::<{b}, _>({value})"),
        IrType::Uint64 => format!("::lean2rust::__facade::to_u64::<{b}, _>({value})"),
        IrType::Usize => format!("::lean2rust::__facade::to_usize::<{b}, _>({value})"),
        IrType::Float => format!("::lean2rust::__facade::to_f64::<{b}, _>({value})"),
        IrType::Float32 => format!("::lean2rust::__facade::to_f32::<{b}, _>({value})"),
        _ => format!("::lean2rust::LeanType::<{b}>::into_lean({value})"),
    }
}

/// Reads a facade value of Rust type `ty` from compiler value `x` of representation `ir`,
/// consuming it when `owned`.
fn from_compiler(ir: IrType, ty: &str, x: &str, owned: bool, b: &str) -> String {
    match ir {
        IrType::Uint8 | IrType::Uint16 => format!("::lean2rust::__facade::from_small::<{b}, {ty}>({x} as usize)"),
        IrType::Uint32 => format!("::lean2rust::__facade::from_u32::<{b}, {ty}>({x})"),
        IrType::Uint64 => format!("::lean2rust::__facade::from_u64::<{b}, {ty}>({x})"),
        IrType::Usize => format!("::lean2rust::__facade::from_usize::<{b}, {ty}>({x})"),
        IrType::Float => format!("::lean2rust::__facade::from_f64::<{b}, {ty}>({x})"),
        IrType::Float32 => format!("::lean2rust::__facade::from_f32::<{b}, {ty}>({x})"),
        _ if owned => format!("::lean2rust::__facade::take::<{b}, {ty}>({x})"),
        _ => format!("<{ty} as ::lean2rust::LeanType<{b}>>::from_lean({x})"),
    }
}

/// Accumulates the generated items of each facade module.
#[derive(Default)]
pub struct ModuleTree {
    pub items: Vec<String>,
    pub children: BTreeMap<String, ModuleTree>,
}

impl ModuleTree {
    pub fn push(&mut self, path: &[String], item: String) {
        let mut node = self;
        for p in path {
            node = node.children.entry(p.clone()).or_default();
        }
        node.items.push(item);
    }

    pub fn render(&self, w: &mut Writer) {
        for item in &self.items {
            for line in item.lines() {
                w.line(line);
            }
        }
        for (name, child) in &self.children {
            w.open(format!("pub mod {name} {{"));
            child.render(w);
            w.close("}");
        }
    }
}

/// The IR parameter list of an export, aligned with its facade parameters.
pub struct FnSpec<'x> {
    pub lean_name: &'x str,
    pub ir_params: &'x [lean2rust_bir::Param],
    pub ir_result: IrType,
    pub type_params: &'x [String],
    pub params: &'x [FacadeParam],
    pub result: &'x FacadeType,
}

impl<'a> Facade<'a> {
    fn generic_names(&self, lean: &[String]) -> Vec<String> {
        let mut taken = BTreeSet::new();
        let mut out = Vec::new();
        for (i, p) in lean.iter().enumerate() {
            let id = type_param_ident(p, i, &taken);
            taken.insert(id.clone());
            out.push(id);
        }
        out
    }

    /// Emits the facade function of `export`.
    pub fn emit_function(
        &mut self,
        modules: &mut ModuleTree,
        export: &Export,
        spec: &FnSpec,
        doc: &[String],
    ) -> Result<(), CodegenError> {
        let placement = self.placement(&export.name)?.clone();
        let depth = placement.module.len();
        let b = self.backend(depth);
        let generics = self.generic_names(spec.type_params);
        if spec.params.len() != spec.ir_params.len() {
            return Err(CodegenError::internal(format!(
                "facade of {} has {} parameters for {} compiled parameters",
                export.name,
                spec.params.len(),
                spec.ir_params.len()
            )));
        }
        let mut w = Writer::new();
        for d in doc {
            w.line(format!("/// {d}"));
        }
        let mut sig = Vec::new();
        let mut arg_names = BTreeSet::new();
        let mut body_pre = Vec::new();
        let mut call_args = Vec::new();
        let mut body_post = Vec::new();
        for (i, (fp, p)) in spec.params.iter().zip(spec.ir_params).enumerate() {
            match fp {
                FacadeParam::Erased => {
                    if p.ty != IrType::Void {
                        call_args.push("rt::lean_box(0)".to_owned());
                    }
                }
                FacadeParam::Value { name, ty } => {
                    if p.ty == IrType::Void || p.ty == IrType::Erased {
                        return Err(CodegenError::internal(format!(
                            "{}: a runtime parameter is compiled as {}",
                            export.name,
                            p.ty.name()
                        )));
                    }
                    let mut arg = snake(if name.is_empty() { "arg" } else { name });
                    arg = arg.trim_start_matches("r#").to_owned();
                    if rust::is_keyword(&arg) || arg == "rt" || arg.starts_with("p_") || arg == "r" {
                        arg = format!("{arg}_");
                    }
                    let mut unique = arg.clone();
                    let mut k = 1;
                    while !arg_names.insert(unique.clone()) {
                        unique = format!("{arg}_{k}");
                        k += 1;
                    }
                    let rty = self.rust_type(ty, depth, &generics)?;
                    sig.push(format!("{unique}: {rty}"));
                    body_pre.push(format!("let p_{i}: {} = {};", rust_type(p.ty), to_compiler(p.ty, &unique, &b)));
                    call_args.push(format!("p_{i}"));
                    if p.ty.is_object() && p.borrow {
                        body_post.push(format!("::lean2rust::__facade::dec::<{b}>(p_{i});"));
                    }
                }
            }
        }
        let ret = self.rust_type(spec.result, depth, &generics)?;
        let bounds = if generics.is_empty() {
            String::new()
        } else {
            format!(
                "<{}>",
                generics.iter().map(|g| format!("{g}: ::lean2rust::LeanType<{b}>")).collect::<Vec<_>>().join(", ")
            )
        };
        let l2r = format!("{}__l2r", "super::".repeat(depth));
        w.line("#[allow(unused_imports, unused_unsafe, clippy::all)]");
        w.open(format!("pub fn {}{bounds}({}) -> {ret} {{", placement.ident, sig.join(", ")));
        w.line("use ::lean2rust::__runtime::{self as rt, Obj};");
        w.line(format!("{l2r}::__initialize();"));
        w.open("unsafe {");
        for l in body_pre {
            w.line(l);
        }
        w.line(format!(
            "let r: {} = {l2r}::{}({});",
            rust_type(spec.ir_result),
            mangle(spec.lean_name),
            call_args.join(", ")
        ));
        for l in body_post {
            w.line(l);
        }
        let result = match spec.result {
            FacadeType::Io(t) => {
                if spec.ir_result.is_scalar() {
                    return Err(CodegenError::internal(format!("{}: IO result compiled as a scalar", export.name)));
                }
                let inner = self.rust_type(t, depth, &generics)?;
                format!("::lean2rust::__facade::take_io::<{b}, {inner}>(r)")
            }
            FacadeType::Eio { error, value } => {
                let e = self.rust_type(error, depth, &generics)?;
                let v = self.rust_type(value, depth, &generics)?;
                format!("::lean2rust::__facade::take_eio::<{b}, {v}, {e}>(r)")
            }
            _ => from_compiler(spec.ir_result, &ret, "r", true, &b),
        };
        w.line(result);
        w.close("}");
        w.close("}");
        w.line("");
        modules.push(&placement.module, w.finish());
        Ok(())
    }

    /// Emits the adapter through which compiled code calls an application-provided Rust
    /// function for extern `decl`. The adapter lives in `__l2r` (depth 1).
    ///
    /// With `export_symbol`, the adapter is an exported C function with that symbol, as Lean's
    /// natively compiled code calls it in `LeanOracle` mode.
    pub fn emit_extern_adapter(
        &mut self,
        w: &mut Writer,
        decl: &lean2rust_bir::Declaration,
        req: &ExternRequirement,
        rust_path: &str,
        export_symbol: Option<&str>,
    ) -> Result<(), CodegenError> {
        let sig: &FacadeSignature = req.facade.as_ref().ok_or_else(|| {
            CodegenError::Extern(format!(
                "cannot generate a Rust adapter for extern {} (`{}`): its Lean type does not determine a Rust signature",
                decl.name,
                req.lean_type.as_deref().unwrap_or("unknown type")
            ))
        })?;
        if sig.params.len() != decl.params.len() {
            return Err(CodegenError::internal(format!("extern {} facade arity mismatch", decl.name)));
        }
        let depth = 1;
        let b = self.backend(depth);
        // Type parameters of a polymorphic extern are instantiated with opaque Lean values.
        let generics: Vec<String> =
            sig.type_params.iter().map(|_| format!("::lean2rust::LeanValue<(), {b}>")).collect();
        let mut params = Vec::new();
        let mut reads = Vec::new();
        let mut args = Vec::new();
        let mut releases = Vec::new();
        for (fp, p) in sig.params.iter().zip(&decl.params) {
            if matches!(p.ty, IrType::Erased | IrType::Void) {
                continue;
            }
            params.push(format!("x_{}: {}", p.var, rust_type(p.ty)));
            match fp {
                FacadeParam::Value { ty, .. } => {
                    let rty = self.rust_type(ty, depth, &generics)?;
                    reads.push(format!(
                        "let v_{}: {rty} = {};",
                        p.var,
                        from_compiler(p.ty, &rty, &format!("x_{}", p.var), false, &b)
                    ));
                    args.push(format!("v_{}", p.var));
                    if p.ty.is_object() && !p.borrow {
                        releases.push(format!("::lean2rust::__facade::dec::<{b}>(x_{});", p.var));
                    }
                }
                FacadeParam::Erased => {
                    if p.ty.is_object() && !p.borrow {
                        releases.push(format!("::lean2rust::__facade::dec::<{b}>(x_{});", p.var));
                    }
                }
            }
        }
        let result = match &sig.result {
            FacadeType::Io(_) => format!("::lean2rust::__facade::make_io::<{b}, _>(r)"),
            FacadeType::Eio { .. } => format!("::lean2rust::__facade::make_eio::<{b}, _, _>(r)"),
            _ => to_compiler(decl.result, "r", &b),
        };
        w.line(format!("// Adapter for extern {} → {rust_path}", decl.name));
        let head = match export_symbol {
            Some(sym) => format!("#[unsafe(export_name = {})]\npub unsafe extern \"C\" fn", rust::string(sym)),
            None => "pub(crate) unsafe fn".to_owned(),
        };
        for (i, line) in head.lines().enumerate() {
            if i + 1 < head.lines().count() {
                w.line(line);
            } else {
                w.open(format!(
                    "{line} a_{}({}) -> {} {{",
                    mangle(&decl.name),
                    params.join(", "),
                    rust_type(decl.result)
                ));
            }
        }
        for l in reads {
            w.line(l);
        }
        for l in releases {
            w.line(l);
        }
        w.line(format!("let r = {rust_path}({});", args.join(", ")));
        w.line(result);
        w.close("}");
        w.line("");
        Ok(())
    }

    /// Declarations of the opaque marker types.
    pub fn emit_markers(&self, w: &mut Writer) {
        w.open("pub mod __opaque {");
        w.line("//! Marker types identifying Lean types exposed as opaque values.");
        for (head, ident) in &self.markers {
            match head {
                Some(h) => w.line(format!("/// Values of Lean type `{h}`.")),
                None => w.line("/// Values of Lean types without a named head constant."),
            }
            w.line(format!("pub enum {ident} {{}}"));
        }
        w.close("}");
    }
}

/// The Lean-facing description of an export used in documentation comments.
pub fn describe_export(export: &Export, source: Option<String>) -> Vec<String> {
    let mut doc = vec![format!("Lean: `{} : {}`", export.name, export.lean_type)];
    if let Some(s) = source {
        doc.push(String::new());
        doc.push(format!("Source: {s}"));
    }
    doc
}
