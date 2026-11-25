//! Generate requests built in memory, for the generators' unit tests: a program named `shapes`
//! whose boundary is what a test gives it.

use crate::c::boundary::{Boundary, CtorDocs, Facility, Function, NamedType};
use crate::core::assurance::{AssuranceDocument, Provenance, SCHEMA_VERSION};
use crate::plugin::{Distribution, GenerateRequest, PROTOCOL_VERSION, ProgramInfo, RuntimeInfo};
use lungo_runtime::wire::{Ctor, Field, FieldKind, Repr, Type, TypeDecl, TypeTable};
use std::collections::BTreeMap;

/// A type of several constructors, `<name>.c<i>`, the fields of each `f<k>` of the given types.
pub fn decl(name: &str, params: u32, ctors: Vec<Vec<Type>>) -> TypeDecl {
    TypeDecl {
        opaque: false,
        name: name.into(),
        params,
        repr: Repr::Object,
        trivial: None,
        ctors: ctors
            .into_iter()
            .enumerate()
            .map(|(i, fields)| Ctor {
                name: format!("{name}.c{i}"),
                tag: i as u32,
                size: fields.len() as u32,
                usize: 0,
                ssize: 0,
                fields: fields
                    .into_iter()
                    .enumerate()
                    .map(|(k, ty)| Field { name: format!("f{k}"), kind: FieldKind::Object(k as u32), ty })
                    .collect(),
            })
            .collect(),
    }
}

/// The type at index `i` of the table, with no type arguments.
pub fn ind(i: u32) -> Type {
    Type::Inductive { index: i, args: vec![] }
}

/// The Lean names of `types`, none of them documented: a type of one constructor is a structure.
pub fn named(types: &[TypeDecl]) -> Vec<NamedType> {
    types
        .iter()
        .map(|d| NamedType {
            lean_name: d.name.clone(),
            params: (0..d.params).map(|k| format!("α{k}")).collect(),
            structure: d.ctors.len() == 1,
            opaque: d.opaque,
            fingerprint: "0".repeat(64),
            doc: None,
            ctors: d.ctors.iter().map(|c| CtorDocs { doc: None, fields: vec![None; c.fields.len()] }).collect(),
        })
        .collect()
}

/// The request of the program `shapes` with these types, functions and facilities.
pub fn request(
    types: Vec<TypeDecl>,
    named: Vec<NamedType>,
    functions: Vec<Function>,
    facilities: Vec<Facility>,
) -> GenerateRequest {
    GenerateRequest {
        protocol_version: PROTOCOL_VERSION,
        program: ProgramInfo {
            name: "shapes".into(),
            lean_version: "4.0.0".into(),
            lean_githash: String::new(),
            bir_version: 1,
            root_modules: vec!["Shapes".into()],
        },
        boundary: Boundary {
            id: "shapes".into(),
            prefix: "shapes__".into(),
            table: TypeTable { types },
            types: named,
            functions,
            facilities,
            async_facilities: vec![],
            run_main: None,
            set_host_extern: "shapes__set_host_extern".into(),
            types_symbol: "shapes__types".into(),
            initialize: "shapes__initialize".into(),
        },
        program_files: BTreeMap::new(),
        runtime: RuntimeInfo {
            version: "1.0.0".into(),
            abi_version: 1,
            distribution: Distribution::Local { dir: "/dist".into() },
        },
        options: BTreeMap::new(),
        extern_types: BTreeMap::new(),
        assurance: AssuranceDocument {
            schema_version: SCHEMA_VERSION,
            program: "shapes".into(),
            provenance: Provenance {
                lean_version: "4.0.0".into(),
                lean_githash: String::new(),
                lungo_version: "1.0.0".into(),
                bir_version: 1,
                runtime_abi: 1,
            },
            library: None,
            specifications: vec![],
            facilities: vec![],
            assumptions: vec![],
            claims: vec![],
            roles: vec![],
            exports: vec![],
        },
    }
}
