//! `lungo.toml`: the Lake project, the settings every language shares (`[lean]`), and each
//! generator's (`[rust]`, `[c]`, `[go]`, `[python]`, `[swift]`, `[ts]`, `[plugins.<name>]`).
//!
//! ```toml
//! project = "lean"
//!
//! [lean]
//! root-modules = ["Formal.Session"]
//! host-externs = ["host_log"]
//!
//! [go]
//! out = "gen/go"
//! options = { package = "formal" }
//! ```

use lungo_build::{Error, LeanOptions, Result, RustOptions};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The built-in binding languages, as named in `--<language>_out` and in `lungo.toml`.
pub const BINDINGS: &[&str] = &["c", "go", "python", "swift", "ts"];

/// A `lungo.toml`.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The Lake project directory, relative to the file.
    pub project: PathBuf,
    #[serde(default)]
    pub lean: LeanOptions,
    #[serde(default)]
    pub rust: RustOptions,
    pub c: Option<Language>,
    pub go: Option<Language>,
    pub python: Option<Language>,
    pub swift: Option<Language>,
    pub ts: Option<Language>,
    /// Generators run as plugins (`lungo-gen-<name>`).
    #[serde(default)]
    pub plugins: BTreeMap<String, Language>,
}

/// A generator's settings: where it writes (relative to the file), its options, and the Lean
/// types other generated packages provide.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    pub out: Option<PathBuf>,
    #[serde(default)]
    pub options: BTreeMap<String, String>,
    #[serde(default, rename = "extern-types")]
    pub extern_types: BTreeMap<String, lungo_build::codegen::plugin::ExternType>,
}

impl ProjectFile {
    /// Reads a `lungo.toml`.
    pub fn load(path: &Path) -> Result<ProjectFile> {
        let text =
            std::fs::read_to_string(path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
        let file: ProjectFile =
            toml::from_str(&text).map_err(|e| Error::Configuration(format!("{}: {e}", path.display())))?;
        for name in file.plugins.keys() {
            if name == "rust" || BINDINGS.contains(&name.as_str()) {
                return Err(Error::Configuration(format!(
                    "{}: `{name}` is a built-in generator, configured in its own [{name}] table, not as a plugin",
                    path.display()
                )));
            }
            validate_language_name(name)?;
        }
        Ok(file)
    }

    /// The settings of binding `language` (built-in or plugin).
    pub fn language(&self, language: &str) -> Option<&Language> {
        match language {
            "c" => self.c.as_ref(),
            "go" => self.go.as_ref(),
            "python" => self.python.as_ref(),
            "swift" => self.swift.as_ref(),
            "ts" => self.ts.as_ref(),
            other => self.plugins.get(other),
        }
    }

    /// Every binding language with settings, built-in ones first.
    pub fn languages(&self) -> Vec<(String, &Language)> {
        let mut out: Vec<(String, &Language)> =
            BINDINGS.iter().filter_map(|l| self.language(l).map(|s| (l.to_string(), s))).collect();
        out.extend(self.plugins.iter().map(|(k, v)| (k.clone(), v)));
        out
    }
}

/// A generator name: lowercase letters, digits and `-`, starting with a letter (it names the
/// plugin program `lungo-gen-<name>`).
pub fn validate_language_name(name: &str) -> Result<()> {
    let valid = name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if valid {
        Ok(())
    } else {
        Err(Error::Configuration(format!(
            "`{name}` cannot name a generator: use lowercase letters, digits and `-`, starting with a letter"
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0083: project files reject unknown keys and plugins named like built ins
    #[test]
    fn test0083_project_files_reject_unknown_keys_and_plugins_named_like_built_ins() {
        let dir = std::env::temp_dir().join(format!("lungo-config-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("lungo.toml");
        std::fs::write(
            &file,
            "project = \"lean\"\n[lean]\nhost-externs = [\"h\"]\n[go]\nout = \"gen/go\"\noptions = { package = \"p\" }\n[plugins.kotlin]\nout = \"k\"\n",
        )
        .unwrap();
        let p = ProjectFile::load(&file).unwrap();
        assert_eq!(p.go.as_ref().unwrap().options["package"], "p");
        assert_eq!(p.languages().iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(), ["go", "kotlin"]);
        std::fs::write(&file, "project = \"lean\"\n[go]\npackage = \"p\"\n").unwrap();
        assert!(ProjectFile::load(&file).is_err(), "options belong in `options`");
        std::fs::write(&file, "project = \"lean\"\n[plugins.go]\nout = \"x\"\n").unwrap();
        assert!(ProjectFile::load(&file).is_err(), "go is built in");
        std::fs::write(&file, "project = \"lean\"\n[plugins.Bad_Name]\nout = \"x\"\n").unwrap();
        assert!(ProjectFile::load(&file).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
