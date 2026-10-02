//! Names of generated API items in the binding languages.
//!
//! A Lean declaration is named by the shortest suffix of its name's components that no other
//! declaration of the same kind ends with (`Demo.Shape.area` is `area` unless another function
//! is also named `…area`). The suffixes are distinct by construction; converting them to a
//! language's case convention can still make two names equal.
//!
//! A function and something that is not a function — a type, a constructor, a descriptor —
//! can come out the same where a language writes both in one case and one namespace
//! (`Gate.acquire` and the type `Acquire` are both `Acquire` in Go). That is how Lean code is
//! written — a type for what an operation answers, named after the operation — so it is not
//! refused: the type keeps the name and the function keeps one more component of its own
//! (`GateAcquire`), by the same rule that separates two functions ending alike.
//!
//! Two functions that convert to one name (`foo` and `Foo`), or two types, are an error
//! naming both declarations: neither has a claim to the shorter name, and a generator never
//! invents a name the user cannot predict.

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
    owners: HashMap<String, Owner>,
    /// Prefixes no claimed name may start with (reserved for generated internals).
    reserved: Vec<String>,
}

/// Who holds a claimed name.
struct Owner {
    /// A description such as "function Demo.add".
    description: String,
    /// Claimed by [`Scope::claim_function`]: a function gives way to anything that is not one.
    function: bool,
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
        self.check_reserved(&name, &owner)?;
        if let Some(other) = self.owners.get(&name) {
            return Err(self.clash(&other.description, &owner, &name));
        }
        self.owners.insert(name.clone(), Owner { description: owner, function: false });
        Ok(name)
    }

    /// Claims the name of the function `lean_name`: `convert` of `short`, its shortest unique
    /// suffix among the functions — or, while that is already the name of something that is
    /// not a function, of the suffix with one more component of its Lean name. The function is
    /// claimed after the types, so it is the function that says whose it is.
    ///
    /// Two functions that convert to one name are an error, as are a function and a
    /// non-function that the function's whole name cannot tell apart.
    pub fn claim_function(
        &mut self,
        lean_name: &str,
        short: &[String],
        convert: impl Fn(&[String]) -> String,
    ) -> Result<String, CodegenError> {
        let owner = format!("function {lean_name}");
        let all = components(lean_name);
        let mut kept = short.len();
        loop {
            let name = convert(&all[all.len() - kept..]);
            self.check_reserved(&name, &owner)?;
            match self.owners.get(&name) {
                None => {
                    self.owners.insert(name.clone(), Owner { description: owner, function: true });
                    return Ok(name);
                }
                Some(other) if other.function || kept == all.len() => {
                    return Err(self.clash(&other.description, &owner, &name));
                }
                Some(_) => kept += 1,
            }
        }
    }

    fn check_reserved(&self, name: &str, owner: &str) -> Result<(), CodegenError> {
        match self.reserved.iter().find(|p| name.starts_with(p.as_str())) {
            Some(p) => Err(CodegenError::Configuration(format!(
                "the {} name `{name}` of {owner} starts with `{p}`, which is reserved for generated internals",
                self.language
            ))),
            None => Ok(()),
        }
    }

    fn clash(&self, other: &str, owner: &str, name: &str) -> CodegenError {
        CodegenError::Configuration(format!(
            "{other} and {owner} would both be named `{name}` in {}; rename one of them in Lean",
            self.language
        ))
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

    /// A function beside a type of its own name keeps one more component, and keeps adding
    /// them while what it gets is taken by something that is not a function. Lean code names
    /// the type an operation answers with after the operation (`Acquire`, `Gate.acquire`); in
    /// a language with one case for both, that was refused, and every such pair had to be
    /// renamed in the model for the sake of one binding.
    #[test]
    fn a_function_gives_way_to_what_is_not_a_function() {
        let upper = |cs: &[String]| -> String {
            cs.iter()
                .map(|c| {
                    let mut chars = c.chars();
                    chars.next().map(|f| f.to_uppercase().chain(chars).collect::<String>()).unwrap_or_default()
                })
                .collect()
        };
        let short = |name: &str| vec![name.rsplit('.').next().unwrap().to_owned()];
        let mut s = Scope::new("Go");
        s.claim("Acquire".into(), "type Demo.Acquire").unwrap();
        s.claim("GateAcquire".into(), "constructor Demo.Gate.acquire").unwrap();

        // One more component is taken too (by a constructor), so the function keeps another.
        assert_eq!(
            s.claim_function("Demo.Gate.acquire", &short("Demo.Gate.acquire"), upper).unwrap(),
            "DemoGateAcquire"
        );
        // A name nothing holds is the shortest suffix, as before.
        assert_eq!(s.claim_function("Demo.Gate.grant", &short("Demo.Gate.grant"), upper).unwrap(), "Grant");

        // Two functions with one name: neither gives way, and both are named.
        let e = s.claim_function("Other.Grant", &short("Other.Grant"), upper).unwrap_err().to_string();
        assert!(e.contains("function Demo.Gate.grant") && e.contains("function Other.Grant"), "{e}");

        // A function whose whole name is a type's cannot be told apart from it.
        let mut whole = Scope::new("Go");
        whole.claim("Acquire".into(), "type Acquire").unwrap();
        let e = whole.claim_function("acquire", &short("acquire"), upper).unwrap_err().to_string();
        assert!(e.contains("type Acquire") && e.contains("function acquire"), "{e}");

        // A type claimed where a function already is: still an error — types come first.
        let e = s.claim("Grant".into(), "type Demo.Grant").unwrap_err().to_string();
        assert!(e.contains("function Demo.Gate.grant") && e.contains("type Demo.Grant"), "{e}");
    }

    #[test]
    fn repeated_locals_are_numbered_without_collisions() {
        assert_eq!(distinct_locals(vec!["x".into(), "y".into(), "x".into()]), vec!["x", "y", "x2"]);
        let clash = distinct_locals(vec!["x".into(), "x".into(), "x2".into()]);
        assert_eq!(clash.len(), 3);
        assert_eq!(clash.iter().collect::<std::collections::HashSet<_>>().len(), 3);
    }
}
