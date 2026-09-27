//! The program's interface: exported functions, the types their signatures reach, and the
//! externs the application implements.

use lungo_protocol::{Export, ExternRequirement, FacadeParam, FacadeType, TypeDecl};
use std::collections::{BTreeSet, HashMap};

/// Names of the described types reachable from the exports' and application externs'
/// signatures, following constructor fields. An opaque type is reached by its head but its
/// fields are not followed: its values are never taken apart. Types the application provides
/// (`is_extern`) are not generated, so the types of their fields are not followed either.
pub fn reachable_types<'t>(
    types: &'t [TypeDecl],
    exports: &[Export],
    externs: &[&ExternRequirement],
    is_extern: &dyn Fn(&str) -> bool,
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
            FacadeType::Opaque { head: Some(h), .. } => out.push(h.clone()),
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
        if is_extern(&n) {
            continue;
        }
        if let Some(t) = by_name.get(n.as_str())
            && seen.insert(t.name.as_str())
            && !t.opaque
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
