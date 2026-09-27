//! Facts about the program's types that the binding generators share.

use lungo_runtime::wire::{Type, TypeTable};

/// For each type of `table`, whether it is a single-constructor type that contains itself by
/// value: through fields, options, pairs, `Except` and other single-constructor types (lists,
/// arrays, functions and types of several constructors hold their values indirectly). Languages
/// whose structures are values (Go, Swift) must break such cycles with a reference.
pub fn value_recursive(table: &TypeTable) -> Vec<bool> {
    let single = |i: u32| table.types.get(i as usize).is_some_and(|d| d.ctors.len() == 1);
    // Edges: type i holds type j by value.
    let edges: Vec<Vec<u32>> = table
        .types
        .iter()
        .map(|d| {
            let mut out = Vec::new();
            if d.ctors.len() == 1 {
                for f in &d.ctors[0].fields {
                    by_value(&f.ty, &single, &mut out);
                }
            }
            out
        })
        .collect();
    (0..table.types.len())
        .map(|start| {
            let mut seen = vec![false; table.types.len()];
            let mut stack: Vec<u32> = edges[start].clone();
            while let Some(j) = stack.pop() {
                if j as usize == start {
                    return true;
                }
                if !std::mem::replace(&mut seen[j as usize], true) {
                    stack.extend(&edges[j as usize]);
                }
            }
            false
        })
        .collect()
}

fn by_value(ty: &Type, single: &dyn Fn(u32) -> bool, out: &mut Vec<u32>) {
    match ty {
        Type::Option(t) => by_value(t, single, out),
        Type::Prod(a, b) | Type::Except { error: a, value: b } => {
            by_value(a, single, out);
            by_value(b, single, out);
        }
        Type::Inductive { index, args } if single(*index) => {
            out.push(*index);
            // A type argument may be stored by value in the structure.
            for a in args {
                by_value(a, single, out);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lungo_runtime::wire::{Ctor, Field, FieldKind, Repr, TypeDecl};

    fn decl(name: &str, ctors: Vec<Vec<Type>>) -> TypeDecl {
        TypeDecl {
            name: name.into(),
            params: 0,
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

    fn ind(i: u32) -> Type {
        Type::Inductive { index: i, args: vec![] }
    }

    #[test]
    fn only_cycles_through_values_count() {
        let table = TypeTable {
            types: vec![
                // 0: structure containing Option 0: recursive by value.
                decl("A", vec![vec![Type::Option(Box::new(ind(0)))]]),
                // 1: structure containing List 1: indirect.
                decl("B", vec![vec![Type::List(Box::new(ind(1)))]]),
                // 2: inductive of two constructors referring to itself: indirect.
                decl("C", vec![vec![], vec![ind(2)]]),
                // 3 and 4: mutually recursive structures through a pair.
                decl("D", vec![vec![Type::Prod(Box::new(Type::Nat), Box::new(ind(4)))]]),
                decl("E", vec![vec![Type::Option(Box::new(ind(3)))]]),
            ],
        };
        assert_eq!(value_recursive(&table), vec![true, false, false, true, true]);
    }
}
