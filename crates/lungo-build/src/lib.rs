//! Build-time integration of Lean projects into Rust crates.
//!
//! A Cargo build script compiles a normal Lake project, in one call when the project's own
//! default targets say what to compile:
//!
//! ```no_run
//! fn main() -> lungo_build::Result<()> {
//!     lungo_build::compile_lean("lean")
//! }
//! ```
//!
//! or with a configured [`Builder`]:
//!
//! ```no_run
//! fn main() -> lungo_build::Result<()> {
//!     lungo_build::configure()
//!         .root_module("Formal.Session")
//!         .type_attribute("Formal", "#[derive(serde::Serialize)]")
//!         .compile_lean("lean")
//! }
//! ```
//!
//! and the crate includes the generated module by the Lake package's name:
//!
//! ```ignore
//! pub mod formal {
//!     lungo::include_lean!("formal");
//! }
//! ```
//!
//! Lake elaborates, kernel-checks, and compiles the Lean project with the exact toolchain it
//! pins; a worker compiled against that toolchain reads Lean's compiler output and hands it to
//! the Rust backend as Bridge IR (the pipeline of `lungo-driver`, which the `lungo` command
//! shares).

mod oracle;

pub use lungo_codegen::rust::Attribute;
pub use lungo_driver::{
    ADAPTER_VERSION, Analysis, BuildKey, CodegenError, Context, Environment, Error, ErrorCode, LeanOptions, Limits,
    RUNTIME_ABI_VERSION, Result, SUPPORTED_TOOLCHAINS, Toolchain, ToolchainPolicy, WorkerCache, bir, cache_root,
    canonical_path, codegen, host_triple, protocol, read_pin, relative_path,
};

use lungo_driver::{claim_output, on_large_stack, output};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How the generated Rust executes the Lean program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Generated Rust on the lungo runtime; no Lean runtime or C code is linked.
    #[default]
    PureRust,
    /// The official Lean native backend and runtime, behind the same facade. Intended as a
    /// reference oracle for differential testing.
    LeanOracle,
}

/// Compiles the Lake project in `project` with the default configuration: the root modules of
/// the project's default targets are compiled and exported, and the generated module is named
/// after the Lake package. Use [`configure`] instead to change anything.
///
/// `project` is relative to the Cargo package. This is the entry point for `build.rs`.
pub fn compile_lean(project: impl AsRef<Path>) -> Result<()> {
    configure().compile_lean(project)
}

/// Configures lungo code generation. Use [`compile_lean`] instead if you do not need to change
/// anything.
pub fn configure() -> Builder {
    Builder::default()
}

/// The Rust generator's settings: the `[rust]` table of a `lungo.toml`.
///
/// Settings that select generated items take a *path*: `.` selects everything, and any other
/// path selects the Lean name it spells and every name in it as a namespace. Fields are named
/// as in `names.json`: `<Structure>.<field>`, `<Constructor>.<binder>`, `<Constructor>#<index>`.
/// A path that selects nothing is a configuration error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct RustOptions {
    pub mode: Mode,
    /// The Rust implementations of the host externs: extern key → Rust path. Its keys are
    /// exactly the host externs of the `[lean]` settings.
    pub rust_externs: BTreeMap<String, String>,
    /// Lean types provided by existing Rust types: Lean type → Rust path.
    pub extern_types: BTreeMap<String, String>,
    pub type_attributes: Vec<Attribute>,
    pub struct_attributes: Vec<Attribute>,
    pub enum_attributes: Vec<Attribute>,
    pub field_attributes: Vec<Attribute>,
    /// Types generated without `#[derive(Debug)]`.
    pub skip_debug: BTreeSet<String>,
    /// Types and functions generated without documentation comments.
    pub disable_comments: BTreeSet<String>,
    pub embed_sources: bool,
    /// The Lean namespace placed at the root of the generated module. Defaults to the first
    /// component of the first root module.
    pub facade_namespace: Option<String>,
    /// Where the generated module is written, as `<out_dir>/<name>/<name>.rs`. Defaults to
    /// `$OUT_DIR/lungo`, where `lungo::include_lean!` finds it; relative paths are relative
    /// to the Cargo package.
    pub out_dir: Option<PathBuf>,
    /// Whether to print `cargo::rerun-if-changed` directives for every input. Defaults to
    /// whether the build runs under Cargo.
    pub emit_rerun_if_changed: Option<bool>,
}

/// Configuration of a Lean project's integration into a Rust crate, built with [`configure`]:
/// the settings every language shares ([`LeanOptions`]) and the Rust generator's
/// ([`RustOptions`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Builder {
    pub lean: LeanOptions,
    pub rust: RustOptions,
}

fn attribute(path: impl AsRef<str>, attribute: impl AsRef<str>) -> Attribute {
    Attribute { path: path.as_ref().to_owned(), attribute: attribute.as_ref().to_owned() }
}

impl Builder {
    /// A builder with these settings.
    pub fn from_options(lean: LeanOptions, rust: RustOptions) -> Self {
        Builder { lean, rust }
    }

    /// Compiles `module` (with everything it imports) instead of the default targets' roots.
    pub fn root_module(mut self, module: impl Into<String>) -> Self {
        self.lean = self.lean.root_module(module);
        self
    }

    /// Generates a public function for the Lean declaration `declaration`.
    pub fn export(mut self, declaration: impl Into<String>) -> Self {
        self.lean = self.lean.export(declaration);
        self
    }

    /// Generates public functions for the definitions of `module` and its submodules.
    pub fn export_module(mut self, module: impl Into<String>) -> Self {
        self.lean = self.lean.export_module(module);
        self
    }

    pub fn mode(mut self, mode: Mode) -> Self {
        self.rust.mode = mode;
        self
    }

    /// Implements the Lean extern `symbol` (or, for `@[extern]` without a symbol, the Lean
    /// declaration name) with the Rust function at `rust_path`, called through a generated
    /// adapter with facade types. The extern becomes a host extern.
    pub fn rust_extern(mut self, symbol: impl Into<String>, rust_path: impl Into<String>) -> Self {
        let symbol = symbol.into();
        self.lean = self.lean.host_extern(symbol.clone());
        self.rust.rust_externs.insert(symbol, rust_path.into());
        self
    }

    /// Uses the existing Rust type at `rust_path` for the Lean type `lean_type` instead of
    /// generating one, typically a type another lungo build generated. The Rust type
    /// implements `lungo::LeanType` for the backend in use and has the Lean type's parameters.
    pub fn extern_type(mut self, lean_type: impl Into<String>, rust_path: impl Into<String>) -> Self {
        self.rust.extern_types.insert(lean_type.into(), rust_path.into());
        self
    }

    /// Adds `attribute` to every generated type (struct or enum) `path` selects.
    pub fn type_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.rust.type_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the generated structs `path` selects.
    pub fn struct_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.rust.struct_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the generated enums `path` selects.
    pub fn enum_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.rust.enum_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the fields of generated types `path` selects.
    pub fn field_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.rust.field_attributes.push(attribute(path, attr));
        self
    }

    /// Generates the types `paths` select without `#[derive(Debug)]`, for the application to
    /// implement `Debug`.
    pub fn skip_debug<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.rust.skip_debug.extend(paths.into_iter().map(|p| p.as_ref().to_owned()));
        self
    }

    /// Generates the types and functions `paths` select without documentation comments.
    pub fn disable_comments<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.rust.disable_comments.extend(paths.into_iter().map(|p| p.as_ref().to_owned()));
        self
    }

    pub fn deny_sorry(mut self, deny: bool) -> Self {
        self.lean = self.lean.deny_sorry(deny);
        self
    }

    pub fn deny_axioms(mut self, deny: bool) -> Self {
        self.lean = self.lean.deny_axioms(deny);
        self
    }

    pub fn deny_unsafe(mut self, deny: bool) -> Self {
        self.lean = self.lean.deny_unsafe(deny);
        self
    }

    pub fn embed_sources(mut self, embed: bool) -> Self {
        self.rust.embed_sources = embed;
        self
    }

    pub fn facade_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.rust.facade_namespace = Some(namespace.into());
        self
    }

    /// Names the generated module, for `lungo::include_lean!`, instead of the Lake package's
    /// name. Letters, digits, `_` and `-`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.lean = self.lean.name(name);
        self
    }

    /// Writes the generated module to `<out_dir>/<name>/<name>.rs` instead of
    /// `$OUT_DIR/lungo/<name>/<name>.rs`. `lungo::include_lean!` finds only the default.
    pub fn out_dir(mut self, out_dir: impl AsRef<Path>) -> Self {
        self.rust.out_dir = Some(out_dir.as_ref().to_path_buf());
        self
    }

    /// Enables or disables printing `cargo::rerun-if-changed` directives for every input.
    /// Enabled by default when the build runs under Cargo.
    pub fn emit_rerun_if_changed(mut self, enable: bool) -> Self {
        self.rust.emit_rerun_if_changed = Some(enable);
        self
    }

    pub fn hermetic_worker_cache(mut self, hermetic: bool) -> Self {
        self.lean = self.lean.hermetic_worker_cache(hermetic);
        self
    }

    pub fn hermetic(mut self, hermetic: bool) -> Self {
        self.lean = self.lean.hermetic(hermetic);
        self
    }

    /// Limits the processor time the worker may use.
    pub fn worker_cpu_limit(mut self, limit: Duration) -> Self {
        self.lean = self.lean.worker_cpu_limit(limit);
        self
    }

    /// Limits the memory, in bytes, the worker may use.
    pub fn worker_memory_limit(mut self, bytes: u64) -> Self {
        self.lean = self.lean.worker_memory_limit(bytes);
        self
    }

    pub fn worker_timeout(mut self, timeout: Duration) -> Self {
        self.lean = self.lean.worker_timeout(timeout);
        self
    }

    pub fn toolchain_policy(mut self, policy: ToolchainPolicy) -> Self {
        self.lean = self.lean.toolchain_policy(policy);
        self
    }

    pub fn toolchain_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.lean = self.lean.toolchain_dir(dir);
        self
    }

    /// Reports at most `n` errors from the worker (`0`, the default, reports all).
    pub fn max_errors(mut self, n: u32) -> Self {
        self.lean = self.lean.max_errors(n);
        self
    }

    pub fn lean_option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.lean = self.lean.lean_option(name, value);
        self
    }

    /// Compiles the Lake project in `project` (relative to the Cargo package) and generates its
    /// Rust module, printing Cargo build-script directives. This is the entry point for
    /// `build.rs`.
    pub fn compile_lean(self, project: impl AsRef<Path>) -> Result<()> {
        let under_cargo = std::env::var_os("CARGO").is_some();
        let manifest_dir = match std::env::var_os("CARGO_MANIFEST_DIR") {
            Some(dir) => PathBuf::from(dir),
            None => std::env::current_dir().map_err(|e| Error::io("cannot determine the current directory", e))?,
        };
        let out_root = match (&self.rust.out_dir, std::env::var_os("OUT_DIR")) {
            (Some(dir), _) => manifest_dir.join(dir),
            (None, Some(out)) => PathBuf::from(out).join("lungo"),
            (None, None) => {
                return Err(Error::Configuration(
                    "OUT_DIR is not set: outside a Cargo build script, choose an output directory with Builder::out_dir"
                        .into(),
                ));
            }
        };
        let (host, target) = match std::env::var("TARGET") {
            Ok(_) => (env_string("HOST")?, cargo_target()?),
            Err(_) => {
                let native = Environment::native(PathBuf::new(), PathBuf::new(), PathBuf::new());
                (native.host, native.target)
            }
        };
        let env = Environment { manifest_dir, work_dir: out_root.join(".work"), out_dir: out_root, host, target };
        let outcome = self.run(project.as_ref(), &env)?;
        if self.rust.emit_rerun_if_changed.unwrap_or(under_cargo) {
            for input in &outcome.inputs {
                println!("cargo::rerun-if-changed={}", input.display());
            }
            for var in ["ELAN_HOME", "LUNGO_CACHE_DIR"] {
                println!("cargo::rerun-if-env-changed={var}");
            }
        }
        if under_cargo {
            for link in &outcome.link_directives {
                println!("{link}");
            }
        }
        Ok(())
    }

    /// Runs the complete pipeline for the Lake project in `project` (relative to
    /// `env.manifest_dir`), publishing the generated module into `env.out_dir/<name>`.
    pub fn run(&self, project: &Path, env: &Environment) -> Result<BuildOutcome> {
        on_large_stack(|| {
            let analysis = std::cell::OnceCell::new();
            self.run_with(project, env, &|ctx| {
                if let Some(a) = analysis.get() {
                    return Ok(a);
                }
                let a = self.lean.analyze_in(ctx, env)?;
                Ok(analysis.get_or_init(|| a))
            })
        })
    }

    /// [`Builder::run`] with the analysis `analysis` provides, called only when the output is
    /// not up to date: a tool generating several outputs from one project analyzes it once.
    pub fn run_with<'a>(
        &self,
        project: &Path,
        env: &Environment,
        analysis: &dyn Fn(&Context) -> Result<&'a Analysis>,
    ) -> Result<BuildOutcome> {
        self.check_externs()?;
        let ctx = self.context(project, env)?;
        claim_output(&ctx)?;
        let config = serde_json::to_string(self).expect("configuration serializes");
        let key = lungo_driver::build_key(&ctx, env, &config)?;
        if let Some(previous) = output::read_build_info(&ctx.out_dir, &ctx.project)
            && previous.build_key == key.value
            && output::inputs_unchanged(&previous)
        {
            return Ok(BuildOutcome {
                name: ctx.name,
                inputs: previous.inputs.iter().map(PathBuf::from).collect(),
                link_directives: previous.link_directives.clone(),
                reused: true,
            });
        }
        let analysis = analysis(&ctx)?;
        let generated = self.generate_with(&ctx, env, analysis)?;
        let inputs: Vec<PathBuf> = analysis.success.input_files.iter().map(|p| ctx.project.join(p)).collect();
        let info = output::BuildInfo::new(&key, &ctx, analysis, &inputs, &generated.link_directives)?;
        output::publish(&ctx.out_dir, &ctx.work_dir, &generated.files, &generated.binary_files, &info)?;
        Ok(BuildOutcome { name: ctx.name, inputs, link_directives: generated.link_directives, reused: false })
    }

    /// Resolves the project, its name and output, the toolchain, and the worker.
    pub fn context(&self, project: &Path, env: &Environment) -> Result<Context> {
        self.lean.context(project, env)
    }

    /// Builds (or verifies) the worker for the project's toolchain.
    pub fn prepare_worker(&self, ctx: &Context, env: &Environment) -> Result<lungo_driver::Worker> {
        self.lean.prepare_worker(ctx, env)
    }

    /// Builds the Lean project in `project` with Lake and runs the worker.
    pub fn analyze(&self, project: &Path, env: &Environment) -> Result<Analysis> {
        self.lean.analyze(project, env)
    }

    /// The Rust implementations of the host externs cover exactly the declared host externs.
    fn check_externs(&self) -> Result<()> {
        let mapped: BTreeSet<&String> = self.rust.rust_externs.keys().collect();
        let declared: BTreeSet<&String> = self.lean.host_externs.iter().collect();
        if mapped == declared {
            return Ok(());
        }
        let unmapped: Vec<&&String> = declared.difference(&mapped).collect();
        let undeclared: Vec<&&String> = mapped.difference(&declared).collect();
        Err(Error::Configuration(format!(
            "the Rust implementations of host externs (`rust-externs`) must map exactly the declared host externs (`host-externs`); unmapped: {unmapped:?}, undeclared: {undeclared:?}"
        )))
    }

    fn facade_namespace_for(&self, analysis: &Analysis) -> Result<String> {
        if let Some(ns) = &self.rust.facade_namespace {
            return Ok(ns.clone());
        }
        let first = analysis
            .success
            .root_modules
            .first()
            .ok_or_else(|| Error::Protocol("the worker reported no root modules".into()))?;
        Ok(first.split('.').next().expect("split yields at least one component").to_owned())
    }

    fn shaping(&self) -> lungo_codegen::Shaping {
        let r = &self.rust;
        lungo_codegen::Shaping {
            type_attributes: r.type_attributes.clone(),
            struct_attributes: r.struct_attributes.clone(),
            enum_attributes: r.enum_attributes.clone(),
            field_attributes: r.field_attributes.clone(),
            skip_debug: r.skip_debug.clone(),
            disable_comments: r.disable_comments.clone(),
            extern_types: r.extern_types.clone(),
        }
    }

    /// Checks trust policies and generates the Rust files for an analysis of `project`.
    pub fn generate(&self, project: &Path, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        self.check_externs()?;
        on_large_stack(|| {
            let ctx = self.context(project, env)?;
            self.generate_with(&ctx, env, analysis)
        })
    }

    fn generate_with(&self, ctx: &Context, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        self.lean.check_trust(&analysis.success)?;
        let embedded =
            if self.rust.embed_sources { Some(output::local_sources(&ctx.project, &analysis.success)?) } else { None };
        let namespace = self.facade_namespace_for(analysis)?;
        let shaping = self.shaping();
        match self.rust.mode {
            Mode::PureRust => {
                let input = lungo_codegen::rust::GenInput {
                    layer: lungo_codegen::rust::Layer::PureRust,
                    success: &analysis.success,
                    toolchain: &analysis.toolchain,
                    facade_namespace: &namespace,
                    aggregate: &ctx.name,
                    rust_externs: &self.rust.rust_externs,
                    local_prefix: &ctx.local_prefix,
                    embedded_sources: embedded.as_ref(),
                    shaping: &shaping,
                };
                let generated = lungo_codegen::rust::generate(&input).map_err(|errors| Error::Codegen {
                    toolchain: format!("v{}", analysis.toolchain.lean_version),
                    bir_version: analysis.success.bir.bir_version,
                    errors,
                })?;
                Ok(Generation { files: generated.files, binary_files: BTreeMap::new(), link_directives: Vec::new() })
            }
            Mode::LeanOracle => oracle::generate(self, ctx, env, analysis, &namespace, &shaping, embedded.as_ref()),
        }
    }
}

/// Generated files and the linker directives they need.
#[derive(Debug, Clone)]
pub struct Generation {
    pub files: BTreeMap<String, String>,
    /// Native artifacts (`LeanOracle` mode), keyed by path relative to the output directory.
    pub binary_files: BTreeMap<String, Vec<u8>>,
    pub link_directives: Vec<String>,
}

/// The result of [`Builder::run`].
#[derive(Debug, Clone)]
pub struct BuildOutcome {
    /// The generated module's name.
    pub name: String,
    /// Files whose changes require regenerating.
    pub inputs: Vec<PathBuf>,
    pub link_directives: Vec<String>,
    /// Whether an up-to-date previous output was kept.
    pub reused: bool,
}

fn env_string(var: &str) -> Result<String> {
    std::env::var(var).map_err(|_| {
        Error::Environment(format!("{var} is not set; Builder::compile_lean must run in a Cargo build script"))
    })
}

fn cargo_target() -> Result<lungo_driver::protocol::Target> {
    use lungo_driver::protocol::{Endian, Target};
    let triple = env_string("TARGET")?;
    let pointer_width: u32 = env_string("CARGO_CFG_TARGET_POINTER_WIDTH")?
        .parse()
        .map_err(|_| Error::Environment("invalid CARGO_CFG_TARGET_POINTER_WIDTH".into()))?;
    let endian = match env_string("CARGO_CFG_TARGET_ENDIAN")?.as_str() {
        "little" => Endian::Little,
        "big" => Endian::Big,
        other => return Err(Error::Environment(format!("unknown target endianness {other:?}"))),
    };
    Ok(Target { triple, pointer_width, endian })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rust_externs_declare_host_externs() {
        let cfg = configure().rust_extern("provider_send", "crate::provider::send");
        assert!(cfg.lean.host_externs.contains("provider_send"));
        assert!(cfg.check_externs().is_ok());
    }

    #[test]
    fn rust_externs_must_cover_exactly_the_host_externs() {
        let mut undeclared = configure();
        undeclared.rust.rust_externs.insert("a".into(), "crate::a".into());
        assert!(matches!(undeclared.check_externs(), Err(Error::Configuration(_))));
        let mut unmapped = configure();
        unmapped.lean.host_externs.insert("b".into());
        assert!(matches!(unmapped.check_externs(), Err(Error::Configuration(_))));
    }

    #[test]
    fn rust_options_round_trip_through_toml_and_reject_unknown_keys() {
        let cfg = configure()
            .type_attribute(".", "#[derive(serde::Serialize)]")
            .field_attribute("Formal.Sess.count", "#[serde(skip)]")
            .skip_debug(["Formal.Secret"]);
        let text = toml::to_string(&cfg.rust).unwrap();
        assert_eq!(toml::from_str::<RustOptions>(&text).unwrap(), cfg.rust);
        assert!(toml::from_str::<RustOptions>("unknown = 1\n").is_err());
        assert_eq!(toml::from_str::<RustOptions>("").unwrap(), RustOptions::default());
    }
}
