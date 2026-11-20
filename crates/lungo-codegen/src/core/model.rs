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

/// For each type of `table`, whether it is a type of several constructors that can contain itself
/// by value once its constructors hold their fields inline: through fields, options, pairs,
/// `Except`, and inductive types and the arguments they are applied to, but not through a type
/// `by_reference` marks (a structure held by reference), lists, arrays or functions. A generic one
/// whose constructors hold one of its type parameters inline counts too, because the argument may
/// be a type that holds it. Swift must mark exactly these enums `indirect`.
pub fn sum_recursive(table: &TypeTable, by_reference: &[bool]) -> Vec<bool> {
    // Edges: type i holds type j inline; and whether type i holds a type parameter inline.
    let mut holds_param = vec![false; table.types.len()];
    let edges: Vec<Vec<u32>> = table
        .types
        .iter()
        .enumerate()
        .map(|(i, d)| {
            let mut out = Vec::new();
            for c in &d.ctors {
                for f in &c.fields {
                    inline(&f.ty, by_reference, &mut out, &mut holds_param[i]);
                }
            }
            out
        })
        .collect();
    (0..table.types.len())
        .map(|start| {
            let d = &table.types[start];
            if d.ctors.len() < 2 {
                return false;
            }
            if d.params > 0 && holds_param[start] {
                return true;
            }
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

fn inline(ty: &Type, by_reference: &[bool], out: &mut Vec<u32>, param: &mut bool) {
    match ty {
        Type::Option(t) => inline(t, by_reference, out, param),
        Type::Prod(a, b) | Type::Except { error: a, value: b } => {
            inline(a, by_reference, out, param);
            inline(b, by_reference, out, param);
        }
        Type::Param(_) => *param = true,
        Type::Inductive { index, args } if !by_reference[*index as usize] => {
            out.push(*index);
            // The type may store an argument inline.
            for a in args {
                inline(a, by_reference, out, param);
            }
        }
        _ => {}
    }
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
            opaque: false,
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

    /// TEST0122: only cycles through values count
    #[test]
    fn test0122_only_cycles_through_values_count() {
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

    /// TEST0400: an inductive of several constructors is recursive when it can hold itself inline
    #[test]
    fn test0400_an_inductive_of_several_constructors_is_recursive_when_it_can_hold_itself_inline() {
        let mut generic = decl("G", vec![vec![], vec![Type::Prod(Box::new(Type::Nat), Box::new(Type::Param(0)))]]);
        generic.params = 1;
        let table = TypeTable {
            types: vec![
                // 0: two constructors holding nothing.
                decl("A", vec![vec![], vec![]]),
                // 1: holds itself through an option.
                decl("B", vec![vec![], vec![Type::Option(Box::new(ind(1)))]]),
                // 2 and 3: two of them holding each other.
                decl("C", vec![vec![], vec![ind(3)]]),
                decl("D", vec![vec![], vec![Type::Except { error: Box::new(Type::String), value: Box::new(ind(2)) }]]),
                // 4 and 5: holds a structure held by reference, which holds it.
                decl("E", vec![vec![], vec![ind(5)]]),
                decl("F", vec![vec![Type::Option(Box::new(ind(4)))]]),
                // 6: holds its parameter inline.
                generic,
                // 7: holds itself as the argument of a type holding its argument inline.
                decl("H", vec![vec![], vec![Type::Inductive { index: 6, args: vec![ind(7)] }]]),
                // 8: holds itself through an array.
                decl("I", vec![vec![], vec![Type::Array(Box::new(ind(8)))]]),
                // 9: a structure holding itself is not an inductive of several constructors.
                decl("J", vec![vec![Type::Option(Box::new(ind(9)))]]),
            ],
        };
        let by_reference = [false, false, false, false, false, true, false, false, false, true];
        assert_eq!(
            sum_recursive(&table, &by_reference),
            vec![false, true, true, true, false, false, true, true, false, false]
        );
    }
}
