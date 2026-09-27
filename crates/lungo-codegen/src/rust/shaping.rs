//! Application control over the shape of the generated facade: attributes on generated types
//! and fields, types provided by other Rust code, and the derives and comments emitted.
//!
//! Every setting selects Lean names with a *path*: `.` selects every name, and any other path
//! selects the Lean name it spells and every name in it as a namespace (`Formal` selects
//! `Formal.Sess` and `Formal.Sess.isOpen`). Fields are named as `names.json` names them:
//! `<Structure>.<field>`, `<Constructor>.<binder>`, or `<Constructor>#<index>`.
//!
//! A path that selects nothing is an error: it cannot have any effect, so it is a mistake.

use crate::CodegenError;
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};

/// An attribute added to the generated items a path selects.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attribute {
    pub path: String,
    /// The attribute as written in Rust, such as `#[derive(serde::Serialize)]`.
    pub attribute: String,
}

/// How the application shapes the generated facade.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Shaping {
    /// Attributes on every generated type (struct or enum) a path selects.
    pub type_attributes: Vec<Attribute>,
    /// Attributes on the generated structs a path selects.
    pub struct_attributes: Vec<Attribute>,
    /// Attributes on the generated enums a path selects.
    pub enum_attributes: Vec<Attribute>,
    /// Attributes on the fields a path selects.
    pub field_attributes: Vec<Attribute>,
    /// Types generated without `#[derive(Debug)]`, for the application to implement `Debug`.
    pub skip_debug: BTreeSet<String>,
    /// Types and functions generated without documentation comments.
    pub disable_comments: BTreeSet<String>,
    /// Lean types provided by existing Rust types instead of generated ones: Lean type name →
    /// Rust path. The Rust type implements `lungo::LeanType` for the backend in use, as the
    /// types lungo generates do.
    pub extern_types: BTreeMap<String, String>,
}

/// Whether `path` selects the Lean name `name`.
pub fn selects(path: &str, name: &str) -> bool {
    path == "." || name == path || name.strip_prefix(path).is_some_and(|rest| rest.starts_with('.'))
}

/// The settings of a [`Shaping`], each recording whether it selected anything.
pub(crate) struct Selector<'a> {
    shaping: &'a Shaping,
    used: RefCell<BTreeSet<(&'static str, String)>>,
}

impl<'a> Selector<'a> {
    pub fn new(shaping: &'a Shaping) -> Self {
        Selector { shaping, used: RefCell::new(BTreeSet::new()) }
    }

    fn matching(&self, setting: &'static str, attributes: &'a [Attribute], name: &str) -> Vec<&'a str> {
        let mut out = Vec::new();
        for a in attributes {
            if selects(&a.path, name) {
                self.used.borrow_mut().insert((setting, a.path.clone()));
                out.push(a.attribute.as_str());
            }
        }
        out
    }

    fn flag(&self, setting: &'static str, paths: &BTreeSet<String>, name: &str) -> bool {
        let mut selected = false;
        for p in paths {
            if selects(p, name) {
                self.used.borrow_mut().insert((setting, p.clone()));
                selected = true;
            }
        }
        selected
    }

    /// Attributes for the generated type of Lean type `name`, which is a struct or an enum.
    pub fn type_attributes(&self, name: &str, is_struct: bool) -> Vec<&'a str> {
        let mut out = self.matching("type_attribute", &self.shaping.type_attributes, name);
        if is_struct {
            out.extend(self.matching("struct_attribute", &self.shaping.struct_attributes, name));
        } else {
            out.extend(self.matching("enum_attribute", &self.shaping.enum_attributes, name));
        }
        out
    }

    /// Attributes for the field with Lean name `name`.
    pub fn field_attributes(&self, name: &str) -> Vec<&'a str> {
        self.matching("field_attribute", &self.shaping.field_attributes, name)
    }

    pub fn skip_debug(&self, name: &str) -> bool {
        self.flag("skip_debug", &self.shaping.skip_debug, name)
    }

    pub fn disable_comments(&self, name: &str) -> bool {
        self.flag("disable_comments", &self.shaping.disable_comments, name)
    }

    /// The Rust path providing Lean type `name`, when the application provides it.
    pub fn extern_type(&self, name: &str) -> Option<&'a str> {
        let path = self.shaping.extern_types.get(name)?;
        self.used.borrow_mut().insert(("extern_type", name.to_owned()));
        Some(path.as_str())
    }

    /// An error for every setting that selected nothing. Paths `.` never count: they select
    /// whatever exists, possibly nothing.
    pub fn unused(&self) -> Vec<CodegenError> {
        let used = self.used.borrow();
        let s = self.shaping;
        let mut settings: Vec<(&'static str, &str)> = Vec::new();
        for (setting, attrs) in [
            ("type_attribute", &s.type_attributes),
            ("struct_attribute", &s.struct_attributes),
            ("enum_attribute", &s.enum_attributes),
            ("field_attribute", &s.field_attributes),
        ] {
            settings.extend(attrs.iter().map(|a| (setting, a.path.as_str())));
        }
        settings.extend(s.skip_debug.iter().map(|p| ("skip_debug", p.as_str())));
        settings.extend(s.disable_comments.iter().map(|p| ("disable_comments", p.as_str())));
        settings.extend(s.extern_types.keys().map(|t| ("extern_type", t.as_str())));
        let mut errors = Vec::new();
        let mut reported = BTreeSet::new();
        for (setting, path) in settings {
            if path != "." && !used.contains(&(setting, path.to_owned())) && reported.insert((setting, path)) {
                errors.push(CodegenError::Configuration(match setting {
                    "extern_type" => format!(
                        "extern_type maps the Lean type `{path}`, which no exported declaration or application extern uses"
                    ),
                    "field_attribute" => format!("{setting} path `{path}` selects no field of a generated type"),
                    "disable_comments" => format!("{setting} path `{path}` selects no generated type or function"),
                    _ => format!("{setting} path `{path}` selects no generated type"),
                }));
            }
        }
        errors
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_select_names_and_namespaces() {
        assert!(selects(".", "Formal.Sess"));
        assert!(selects("Formal.Sess", "Formal.Sess"));
        assert!(selects("Formal", "Formal.Sess.isOpen"));
        assert!(!selects("Formal.Se", "Formal.Sess"), "a path is not a string prefix");
        assert!(!selects("Formal.Sess.isOpen", "Formal.Sess"));
    }

    #[test]
    fn settings_that_select_nothing_are_reported() {
        let shaping = Shaping {
            type_attributes: vec![
                Attribute { path: ".".into(), attribute: "#[a]".into() },
                Attribute { path: "Formal.Missing".into(), attribute: "#[b]".into() },
            ],
            skip_debug: ["Formal.Sess".to_owned()].into(),
            extern_types: [("Other.T".to_owned(), "crate::T".to_owned())].into(),
            ..Shaping::default()
        };
        let s = Selector::new(&shaping);
        assert_eq!(s.type_attributes("Formal.Sess", true), ["#[a]"]);
        assert!(s.skip_debug("Formal.Sess"));
        let unused: Vec<String> = s.unused().iter().map(|e| e.to_string()).collect();
        assert_eq!(unused.len(), 2, "{unused:?}");
        assert!(unused[0].contains("Formal.Missing"));
        assert!(unused[1].contains("Other.T"));
    }
}
