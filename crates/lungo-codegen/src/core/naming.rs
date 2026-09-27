//! Names of generated API items in the binding languages.
//!
//! A Lean declaration is named by the shortest suffix of its name's components that no other
//! declaration of the same kind ends with (`Demo.Shape.area` is `area` unless another function
//! is also named `…area`). The suffixes are distinct by construction; converting them to a
//! language's case convention can still make two names equal (`foo` and `Foo`), which is an
//! error naming both declarations: a generator never invents a name the user cannot predict.

use super::names::components;
use crate::CodegenError;
use std::collections::HashMap;

/// For each of `names` (distinct canonical Lean names), the components of its shortest unique
/// suffix.
pub fn short_names(names: &[&str]) -> Vec<Vec<String>> {
    let split: Vec<Vec<String>> = names.iter().map(|n| components(n)).collect();
    split
        .iter()
        .enumerate()
        .map(|(i, cs)| {
            for k in 1..=cs.len() {
                let suffix = &cs[cs.len() - k..];
                let shared = split.iter().enumerate().any(|(j, other)| j != i && other.ends_with(suffix));
                if !shared {
                    return suffix.to_vec();
                }
            }
            cs.clone()
        })
        .collect()
}

/// A namespace of generated identifiers: claiming a name twice is an error naming both owners.
pub struct Scope {
    language: &'static str,
    owners: HashMap<String, String>,
    /// Prefixes no claimed name may start with (reserved for generated internals).
    reserved: Vec<String>,
}

impl Scope {
    pub fn new(language: &'static str) -> Scope {
        Scope { language, owners: HashMap::new(), reserved: Vec::new() }
    }

    /// Reserves the identifiers starting with `prefix`.
    pub fn reserve_prefix(&mut self, prefix: impl Into<String>) {
        self.reserved.push(prefix.into());
    }

    /// Claims `name` for `owner` (a description such as "function Demo.add").
    pub fn claim(&mut self, name: String, owner: impl Into<String>) -> Result<String, CodegenError> {
        let owner = owner.into();
        if let Some(p) = self.reserved.iter().find(|p| name.starts_with(p.as_str())) {
            return Err(CodegenError::Configuration(format!(
                "the {} name `{name}` of {owner} starts with `{p}`, which is reserved for generated internals",
                self.language
            )));
        }
        if let Some(other) = self.owners.get(&name) {
            return Err(CodegenError::Configuration(format!(
                "{other} and {owner} would both be named `{name}` in {}; rename one of them in Lean",
                self.language
            )));
        }
        self.owners.insert(name.clone(), owner);
        Ok(name)
    }
}

/// Local names (parameters of one function): made distinct by numbering repeats.
pub fn distinct_locals(names: Vec<String>) -> Vec<String> {
    let mut seen: HashMap<String, usize> = HashMap::new();
    let mut out = Vec::new();
    for n in names {
        let count = seen.entry(n.clone()).or_insert(0);
        *count += 1;
        out.push(if *count == 1 { n } else { format!("{n}{count}") });
    }
    // A numbered name can equal another parameter's name (`x2` and a second `x`).
    let mut all: HashMap<String, usize> = HashMap::new();
    for n in &out {
        *all.entry(n.clone()).or_insert(0) += 1;
    }
    if all.values().any(|c| *c > 1) {
        return out.iter().enumerate().map(|(i, n)| format!("{n}_{i}")).collect();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortest_unique_suffixes_are_distinct() {
        let names = ["Demo.area", "Geo.Shape.area", "Demo.perimeter", "B", "A.B"];
        let short = short_names(&names);
        assert_eq!(short[0], vec!["Demo", "area"]);
        assert_eq!(short[1], vec!["Shape", "area"]);
        assert_eq!(short[2], vec!["perimeter"]);
        assert_eq!(short[3], vec!["B"]);
        assert_eq!(short[4], vec!["A", "B"]);
    }

    #[test]
    fn a_name_claimed_twice_is_an_error_naming_both_owners() {
        let mut s = Scope::new("C");
        s.reserve_prefix("demo__");
        s.claim("demo_foo".into(), "function Demo.foo").unwrap();
        let e = s.claim("demo_foo".into(), "function Demo.Foo").unwrap_err().to_string();
        assert!(e.contains("Demo.foo") && e.contains("Demo.Foo"), "{e}");
        assert!(s.claim("demo__x".into(), "type X").is_err());
    }

    #[test]
    fn repeated_locals_are_numbered_without_collisions() {
        assert_eq!(distinct_locals(vec!["x".into(), "y".into(), "x".into()]), vec!["x", "y", "x2"]);
        let clash = distinct_locals(vec!["x".into(), "x".into(), "x2".into()]);
        assert_eq!(clash.len(), 3);
        assert_eq!(clash.iter().collect::<std::collections::HashSet<_>>().len(), 3);
    }
}
