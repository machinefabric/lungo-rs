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
//! the Rust backend as Bridge IR. The `cargo lungo` command uses this same library.

mod error;
mod fingerprint;
mod lake;
mod oracle;
mod output;
mod toolchain;
mod worker;

pub use error::{Error, Result};
pub use lungo_codegen::{Attribute, CodegenError, ErrorCode};

// The crates whose types appear in this crate's API (`Analysis`, `Error::Codegen`), for tools
// built on the pipeline such as `cargo lungo`: through these, they use exactly the versions
// this crate was built with.
/// The Bridge IR: the compiled Lean program the worker reports, and its verifier.
pub use lungo_bir as bir;
/// The Rust backend, and the conventions of the files it generates.
pub use lungo_codegen as codegen;
/// The protocol between this crate and its Lean worker, including the worker's analysis.
pub use lungo_protocol as protocol;
pub use toolchain::{SUPPORTED_TOOLCHAINS, Toolchain, ToolchainPolicy, read_pin};
pub use worker::{ADAPTER_VERSION, Limits, WorkerCache};

use fingerprint::Hasher;
use lungo_protocol::{
    CompilerOption, DiagnosticOptions, Endian, ExportPolicy, Outcome, PROTOCOL_VERSION, Request, Response, Roots,
    Success, Target,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
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

/// Configuration of a Lean project's integration, built with [`configure`]. Also loadable, in
/// the `[build]` table of a `lungo.toml`, by `cargo lungo`.
///
/// Settings that select generated items take a *path*: `.` selects everything, and any other
/// path selects the Lean name it spells and every name in it as a namespace. Fields are named
/// as in `names.json`: `<Structure>.<field>`, `<Constructor>.<binder>`, `<Constructor>#<index>`.
/// A path that selects nothing is a configuration error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Builder {
    /// Modules whose code is compiled. Empty: the root modules of the Lake package's default
    /// targets, as Lake resolves them.
    pub root_modules: Vec<String>,
    /// Declarations that receive public facades.
    pub exports: Vec<String>,
    /// Modules (and their submodules) whose declarations receive public facades. When neither
    /// this nor `exports` names anything, the root modules are exported.
    pub export_modules: Vec<String>,
    pub mode: Mode,
    /// Application-provided implementations of extern symbols: symbol → Rust path.
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
    pub deny_sorry: bool,
    pub deny_axioms: bool,
    pub deny_unsafe: bool,
    pub embed_sources: bool,
    /// The Lean namespace placed at the root of the generated module. Defaults to the first
    /// component of the first root module.
    pub facade_namespace: Option<String>,
    /// The name of the generated module, used by `lungo::include_lean!`. Defaults to the Lake
    /// package's name.
    pub name: Option<String>,
    /// Where the generated module is written, as `<out_dir>/<name>/<name>.rs`. Defaults to
    /// `$OUT_DIR/lungo`, where `lungo::include_lean!` finds it; relative paths are relative
    /// to the Cargo package.
    pub out_dir: Option<PathBuf>,
    /// Whether to print `cargo::rerun-if-changed` directives for every input. Defaults to
    /// whether the build runs under Cargo.
    pub emit_rerun_if_changed: Option<bool>,
    /// Keep compiled workers in the build's work directory instead of the shared cache.
    pub hermetic_worker_cache: bool,
    /// Run the worker with a minimal environment.
    pub hermetic: bool,
    /// Seconds after which a running worker is terminated.
    pub worker_timeout: Option<u64>,
    /// Processor seconds the worker may use (see [`Limits`]).
    pub worker_cpu_limit: Option<u64>,
    /// Bytes of memory the worker may use (see [`Limits`]); an error on platforms that
    /// cannot enforce it.
    pub worker_memory_limit: Option<u64>,
    pub install_toolchain: bool,
    /// Use this toolchain installation instead of the one elan manages.
    pub toolchain_dir: Option<PathBuf>,
    /// Lean options in effect while the worker runs Lean metaprograms.
    pub lean_options: BTreeMap<String, String>,
    /// Report at most this many errors from the worker; `0` reports all.
    pub max_errors: u32,
}

impl Default for Builder {
    fn default() -> Self {
        Builder {
            root_modules: Vec::new(),
            exports: Vec::new(),
            export_modules: Vec::new(),
            mode: Mode::PureRust,
            rust_externs: BTreeMap::new(),
            extern_types: BTreeMap::new(),
            type_attributes: Vec::new(),
            struct_attributes: Vec::new(),
            enum_attributes: Vec::new(),
            field_attributes: Vec::new(),
            skip_debug: BTreeSet::new(),
            disable_comments: BTreeSet::new(),
            deny_sorry: true,
            deny_axioms: false,
            deny_unsafe: false,
            embed_sources: false,
            facade_namespace: None,
            name: None,
            out_dir: None,
            emit_rerun_if_changed: None,
            hermetic_worker_cache: false,
            hermetic: false,
            worker_timeout: None,
            worker_cpu_limit: None,
            worker_memory_limit: None,
            install_toolchain: false,
            toolchain_dir: None,
            lean_options: BTreeMap::new(),
            max_errors: 0,
        }
    }
}

/// A `lungo.toml`: the Lake project of a Cargo package and its [`Builder`] settings, for
/// `cargo lungo`.
///
/// ```toml
/// project = "lean"
///
/// [build]
/// root-modules = ["Formal.Session"]
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectFile {
    /// The Lake project directory, relative to the Cargo package.
    pub project: PathBuf,
    /// The build settings; every one has a default.
    #[serde(default)]
    pub build: Builder,
}

impl ProjectFile {
    /// Reads a `lungo.toml`.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
        toml::from_str(&text).map_err(|e| Error::Configuration(format!("{}: {e}", path.display())))
    }
}

fn attribute(path: impl AsRef<str>, attribute: impl AsRef<str>) -> Attribute {
    Attribute { path: path.as_ref().to_owned(), attribute: attribute.as_ref().to_owned() }
}

impl Builder {
    /// Compiles `module` (with everything it imports) instead of the default targets' roots.
    pub fn root_module(mut self, module: impl Into<String>) -> Self {
        self.root_modules.push(module.into());
        self
    }

    /// Generates a public function for the Lean declaration `declaration`.
    pub fn export(mut self, declaration: impl Into<String>) -> Self {
        self.exports.push(declaration.into());
        self
    }

    /// Generates public functions for the definitions of `module` and its submodules.
    pub fn export_module(mut self, module: impl Into<String>) -> Self {
        self.export_modules.push(module.into());
        self
    }

    pub fn mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }

    /// Implements the Lean extern `symbol` (or, for `@[extern]` without a symbol, the Lean
    /// declaration name) with the Rust function at `rust_path`, called through a generated
    /// adapter with facade types.
    pub fn rust_extern(mut self, symbol: impl Into<String>, rust_path: impl Into<String>) -> Self {
        self.rust_externs.insert(symbol.into(), rust_path.into());
        self
    }

    /// Uses the existing Rust type at `rust_path` for the Lean type `lean_type` instead of
    /// generating one, typically a type another lungo build generated. The Rust type
    /// implements `lungo::LeanType` for the backend in use and has the Lean type's parameters.
    pub fn extern_type(mut self, lean_type: impl Into<String>, rust_path: impl Into<String>) -> Self {
        self.extern_types.insert(lean_type.into(), rust_path.into());
        self
    }

    /// Adds `attribute` to every generated type (struct or enum) `path` selects.
    pub fn type_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.type_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the generated structs `path` selects.
    pub fn struct_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.struct_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the generated enums `path` selects.
    pub fn enum_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.enum_attributes.push(attribute(path, attr));
        self
    }

    /// Adds `attribute` to the fields of generated types `path` selects.
    pub fn field_attribute(mut self, path: impl AsRef<str>, attr: impl AsRef<str>) -> Self {
        self.field_attributes.push(attribute(path, attr));
        self
    }

    /// Generates the types `paths` select without `#[derive(Debug)]`, for the application to
    /// implement `Debug`.
    pub fn skip_debug<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.skip_debug.extend(paths.into_iter().map(|p| p.as_ref().to_owned()));
        self
    }

    /// Generates the types and functions `paths` select without documentation comments.
    pub fn disable_comments<I, S>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.disable_comments.extend(paths.into_iter().map(|p| p.as_ref().to_owned()));
        self
    }

    pub fn deny_sorry(mut self, deny: bool) -> Self {
        self.deny_sorry = deny;
        self
    }

    pub fn deny_axioms(mut self, deny: bool) -> Self {
        self.deny_axioms = deny;
        self
    }

    pub fn deny_unsafe(mut self, deny: bool) -> Self {
        self.deny_unsafe = deny;
        self
    }

    pub fn embed_sources(mut self, embed: bool) -> Self {
        self.embed_sources = embed;
        self
    }

    pub fn facade_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.facade_namespace = Some(namespace.into());
        self
    }

    /// Names the generated module, for `lungo::include_lean!`, instead of the Lake package's
    /// name. Letters, digits, `_` and `-`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Writes the generated module to `<out_dir>/<name>/<name>.rs` instead of
    /// `$OUT_DIR/lungo/<name>/<name>.rs`. `lungo::include_lean!` finds only the default.
    pub fn out_dir(mut self, out_dir: impl AsRef<Path>) -> Self {
        self.out_dir = Some(out_dir.as_ref().to_path_buf());
        self
    }

    /// Enables or disables printing `cargo::rerun-if-changed` directives for every input.
    /// Enabled by default when the build runs under Cargo.
    pub fn emit_rerun_if_changed(mut self, enable: bool) -> Self {
        self.emit_rerun_if_changed = Some(enable);
        self
    }

    pub fn hermetic_worker_cache(mut self, hermetic: bool) -> Self {
        self.hermetic_worker_cache = hermetic;
        self
    }

    pub fn hermetic(mut self, hermetic: bool) -> Self {
        self.hermetic = hermetic;
        self
    }

    /// Limits the processor time the worker may use.
    pub fn worker_cpu_limit(mut self, limit: Duration) -> Self {
        self.worker_cpu_limit = Some(limit.as_secs().max(1));
        self
    }

    /// Limits the memory, in bytes, the worker may use.
    pub fn worker_memory_limit(mut self, bytes: u64) -> Self {
        self.worker_memory_limit = Some(bytes);
        self
    }

    pub fn worker_timeout(mut self, timeout: Duration) -> Self {
        self.worker_timeout = Some(timeout.as_secs().max(1));
        self
    }

    pub fn toolchain_policy(mut self, policy: ToolchainPolicy) -> Self {
        self.install_toolchain = policy == ToolchainPolicy::Install;
        self
    }

    pub fn toolchain_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.toolchain_dir = Some(dir.into());
        self
    }

    /// Reports at most `n` errors from the worker (`0`, the default, reports all).
    pub fn max_errors(mut self, n: u32) -> Self {
        self.max_errors = n;
        self
    }

    pub fn lean_option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.lean_options.insert(name.into(), value.into());
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
        let out_root = match (&self.out_dir, std::env::var_os("OUT_DIR")) {
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
        if self.emit_rerun_if_changed.unwrap_or(under_cargo) {
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
        on_large_stack(|| self.run_here(project, env))
    }

    fn run_here(&self, project: &Path, env: &Environment) -> Result<BuildOutcome> {
        let ctx = self.context(project, env)?;
        claim_output(&ctx)?;
        let key = self.build_key(&ctx, env)?;
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
        let analysis = self.analyze_with(&ctx, env)?;
        let generated = self.generate_with(&ctx, env, &analysis)?;
        let inputs: Vec<PathBuf> = analysis.success.input_files.iter().map(|p| ctx.project.join(p)).collect();
        let info = output::BuildInfo::new(&key, &ctx, &analysis, &inputs, &generated.link_directives)?;
        output::publish(&ctx.out_dir, &ctx.work_dir, &generated.files, &generated.binary_files, &info)?;
        Ok(BuildOutcome { name: ctx.name, inputs, link_directives: generated.link_directives, reused: false })
    }

    /// Resolves the project, its name and output, the toolchain, and the worker.
    pub fn context(&self, project: &Path, env: &Environment) -> Result<Context> {
        let project_path = if project.is_absolute() { project.to_path_buf() } else { env.manifest_dir.join(project) };
        let project = canonical_path(&project_path)
            .map_err(|e| Error::io(format!("cannot resolve the Lean project {}", project_path.display()), e))?;
        let pin = toolchain::read_pin(&project)?;
        let lake_project = lake::validate_project(&project)?;
        let name = match &self.name {
            Some(name) => name.clone(),
            None => lake_project.name,
        };
        validate_name(&name, self.name.is_none())?;
        let policy = if self.install_toolchain { ToolchainPolicy::Install } else { ToolchainPolicy::Strict };
        let toolchain = toolchain::resolve(&pin, self.toolchain_dir.as_deref(), policy)?;
        let work_dir = env.work_dir.join(&name);
        let cache = if self.hermetic_worker_cache {
            WorkerCache::Hermetic(work_dir.join("worker-cache"))
        } else {
            WorkerCache::Shared(worker::default_shared_cache()?)
        };
        let worker_identity = worker::identity(&toolchain, &env.host);
        let package = canonical_path(&env.manifest_dir)
            .map_err(|e| Error::io(format!("cannot resolve the package {}", env.manifest_dir.display()), e))?;
        let local_prefix = relative_path(&package, &project);
        Ok(Context {
            project,
            out_dir: env.out_dir.join(&name),
            work_dir,
            name,
            toolchain,
            cache,
            worker_identity,
            local_prefix,
        })
    }

    /// Builds (or verifies) the worker for the project's toolchain.
    pub fn prepare_worker(&self, ctx: &Context, env: &Environment) -> Result<worker::Worker> {
        worker::prepare(&ctx.toolchain, &ctx.cache, &env.host)
    }

    fn roots(&self) -> Roots {
        if self.root_modules.is_empty() { Roots::DefaultTargets } else { Roots::Modules(self.root_modules.clone()) }
    }

    fn request(&self, ctx: &Context, env: &Environment) -> Request {
        Request {
            protocol_version: PROTOCOL_VERSION,
            bridge_version: env!("CARGO_PKG_VERSION").into(),
            project_root: ctx.project.to_string_lossy().into_owned(),
            roots: self.roots(),
            export_policy: ExportPolicy {
                declarations: self.exports.clone(),
                modules: self.export_modules.clone(),
                roots: self.exports.is_empty() && self.export_modules.is_empty(),
            },
            host_triple: env.host.clone(),
            target: env.target.clone(),
            compiler_options: self
                .lean_options
                .iter()
                .map(|(name, value)| CompilerOption { name: name.clone(), value: value.clone() })
                .collect(),
            hermetic: self.hermetic,
            diagnostics: DiagnosticOptions { max_errors: self.max_errors },
            runtime_exports: lungo_runtime::exports::REQUIRED.iter().map(|e| e.symbol.to_owned()).collect(),
        }
    }

    /// Builds the Lean project in `project` with Lake and runs the worker.
    pub fn analyze(&self, project: &Path, env: &Environment) -> Result<Analysis> {
        on_large_stack(|| {
            let ctx = self.context(project, env)?;
            self.analyze_with(&ctx, env)
        })
    }

    fn analyze_with(&self, ctx: &Context, env: &Environment) -> Result<Analysis> {
        let worker = self.prepare_worker(ctx, env)?;
        lake::build(&ctx.toolchain, &ctx.project, &self.roots(), true)?;
        let request = self.request(ctx, env);
        let response = worker::run(
            &worker,
            &ctx.toolchain,
            &request,
            &worker::RunOptions {
                project: &ctx.project,
                scratch: &ctx.work_dir,
                timeout: self.worker_timeout.map(Duration::from_secs),
                limits: worker::Limits {
                    cpu: self.worker_cpu_limit.map(Duration::from_secs),
                    memory: self.worker_memory_limit,
                },
                hermetic: self.hermetic,
            },
        )?;
        let Response { toolchain, diagnostics, outcome, .. } = response;
        match outcome {
            Outcome::Failure => Err(Error::Worker { toolchain: format!("v{}", toolchain.lean_version), diagnostics }),
            Outcome::Success(success) => Ok(Analysis { success: *success, toolchain, warnings: diagnostics }),
        }
    }

    fn facade_namespace_for(&self, analysis: &Analysis) -> Result<String> {
        if let Some(ns) = &self.facade_namespace {
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
        lungo_codegen::Shaping {
            type_attributes: self.type_attributes.clone(),
            struct_attributes: self.struct_attributes.clone(),
            enum_attributes: self.enum_attributes.clone(),
            field_attributes: self.field_attributes.clone(),
            skip_debug: self.skip_debug.clone(),
            disable_comments: self.disable_comments.clone(),
            extern_types: self.extern_types.clone(),
        }
    }

    /// Checks trust policies and generates the Rust files for an analysis of `project`.
    pub fn generate(&self, project: &Path, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        on_large_stack(|| {
            let ctx = self.context(project, env)?;
            self.generate_with(&ctx, env, analysis)
        })
    }

    fn generate_with(&self, ctx: &Context, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        self.check_trust(&analysis.success)?;
        let embedded =
            if self.embed_sources { Some(output::local_sources(&ctx.project, &analysis.success)?) } else { None };
        let namespace = self.facade_namespace_for(analysis)?;
        let shaping = self.shaping();
        match self.mode {
            Mode::PureRust => {
                let input = lungo_codegen::GenInput {
                    layer: lungo_codegen::Layer::PureRust,
                    success: &analysis.success,
                    toolchain: &analysis.toolchain,
                    facade_namespace: &namespace,
                    aggregate: &ctx.name,
                    rust_externs: &self.rust_externs,
                    local_prefix: &ctx.local_prefix,
                    embedded_sources: embedded.as_ref(),
                    shaping: &shaping,
                };
                let generated = lungo_codegen::generate(&input).map_err(|errors| Error::Codegen {
                    toolchain: format!("v{}", analysis.toolchain.lean_version),
                    bir_version: analysis.success.bir.bir_version,
                    errors,
                })?;
                Ok(Generation { files: generated.files, binary_files: BTreeMap::new(), link_directives: Vec::new() })
            }
            Mode::LeanOracle => oracle::generate(self, ctx, env, analysis, &namespace, &shaping, embedded.as_ref()),
        }
    }

    fn check_trust(&self, success: &Success) -> Result<()> {
        const STANDARD: &[&str] = &["propext", "Classical.choice", "Quot.sound"];
        let mut violations = Vec::new();
        for e in &success.interface.exports {
            if self.deny_sorry && e.trust.depends_on_sorry {
                violations.push(format!("{} depends on `sorry` (deny_sorry)", e.name));
            }
            if self.deny_axioms {
                let extra: Vec<&String> = e.trust.axioms.iter().filter(|a| !STANDARD.contains(&a.as_str())).collect();
                if !extra.is_empty() {
                    violations.push(format!(
                        "{} depends on non-standard axioms {} (deny_axioms)",
                        e.name,
                        extra.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                    ));
                }
            }
            if self.deny_unsafe && !e.trust.unsafe_dependencies.is_empty() {
                violations.push(format!(
                    "{} depends on unsafe definitions {} (deny_unsafe)",
                    e.name,
                    e.trust.unsafe_dependencies.join(", ")
                ));
            }
        }
        if violations.is_empty() { Ok(()) } else { Err(Error::Trust(violations)) }
    }

    fn build_key(&self, ctx: &Context, env: &Environment) -> Result<BuildKey> {
        let mut h = Hasher::new("build-key");
        let pin = toolchain::read_pin(&ctx.project)?;
        h.str(&pin)
            .str(&ctx.toolchain.version)
            .str(&ctx.toolchain.githash)
            .str(&ctx.toolchain.sysroot_identity)
            .field(&lungo_bir::BIR_VERSION.to_le_bytes())
            .field(&ADAPTER_VERSION.to_le_bytes())
            .str(env!("CARGO_PKG_VERSION"))
            .field(&lungo_runtime::ABI_VERSION.to_le_bytes())
            .str(&ctx.worker_identity)
            .str(&env.host)
            .str(&env.target.triple)
            .field(&env.target.pointer_width.to_le_bytes())
            .str(match env.target.endian {
                Endian::Little => "little",
                Endian::Big => "big",
            })
            .str(&ctx.local_prefix)
            .str(&ctx.name);
        for (path, digest) in lake::snapshot(&ctx.project)? {
            h.str(&relative_path(&ctx.project, &path)).str(&digest);
        }
        let config = serde_json::to_string(self).expect("configuration serializes");
        h.str(&config);
        // The generator itself: the running executable links lungo-build, the code
        // generator, and the runtime registry, so any change to them changes the key.
        let exe = std::env::current_exe().map_err(|e| Error::io("cannot locate the running generator", e))?;
        let exe_bytes = std::fs::read(&exe).map_err(|e| Error::io(format!("cannot read {}", exe.display()), e))?;
        h.str(&fingerprint::hash_bytes(&exe_bytes));
        Ok(BuildKey { value: h.finish() })
    }
}

/// Checks that `name` can name the generated module: it is a directory and file name, and the
/// string `lungo::include_lean!` is given.
fn validate_name(name: &str, from_package: bool) -> Result<()> {
    let valid = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.starts_with('-');
    if valid {
        return Ok(());
    }
    Err(Error::Configuration(if from_package {
        format!(
            "the Lake package name {name:?} cannot name the generated module (letters, digits, `_` and `-`); choose one with Builder::name"
        )
    } else {
        format!("the module name {name:?} is invalid: use letters, digits, `_` and `-`")
    }))
}

/// Output directories written by this process, and the projects that wrote them: two builds in
/// one build script must not publish into the same directory.
static CLAIMED_OUTPUTS: Mutex<BTreeMap<PathBuf, PathBuf>> = Mutex::new(BTreeMap::new());

fn claim_output(ctx: &Context) -> Result<()> {
    let mut claimed = CLAIMED_OUTPUTS.lock().unwrap_or_else(|p| p.into_inner());
    match claimed.get(&ctx.out_dir) {
        Some(owner) if owner != &ctx.project => Err(Error::Configuration(format!(
            "the Lean projects {} and {} would both generate the module `{}`; give one another name with Builder::name",
            owner.display(),
            ctx.project.display(),
            ctx.name
        ))),
        _ => {
            claimed.insert(ctx.out_dir.clone(), ctx.project.clone());
            Ok(())
        }
    }
}

/// Where and for what a build runs.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The Cargo package directory; relative project paths are resolved against it.
    pub manifest_dir: PathBuf,
    /// The directory generated modules are published into, each as `<out_dir>/<name>`.
    pub out_dir: PathBuf,
    /// Scratch space for worker requests and staged output, per module in `<work_dir>/<name>`.
    pub work_dir: PathBuf,
    pub host: String,
    pub target: Target,
}

impl Environment {
    /// An environment for the machine running the tool: host and target are the same.
    pub fn native(manifest_dir: PathBuf, out_dir: PathBuf, work_dir: PathBuf) -> Self {
        let host = host_triple();
        Environment {
            manifest_dir,
            out_dir,
            work_dir,
            host: host.clone(),
            target: Target {
                triple: host,
                pointer_width: usize::BITS,
                endian: if cfg!(target_endian = "big") { Endian::Big } else { Endian::Little },
            },
        }
    }
}

/// The host triple lungo-build itself was compiled for.
pub fn host_triple() -> String {
    env!("LUNGO_BUILD_HOST").to_owned()
}

/// The resolved project, generated module, toolchain, and worker cache.
#[derive(Debug, Clone)]
pub struct Context {
    pub project: PathBuf,
    /// The generated module's name, used by `lungo::include_lean!`.
    pub name: String,
    /// Where the module is published: `<out_dir>/<name>`.
    pub out_dir: PathBuf,
    /// Scratch space for this module.
    pub work_dir: PathBuf,
    pub toolchain: Toolchain,
    pub cache: WorkerCache,
    pub worker_identity: String,
    /// The project directory relative to the Cargo package, `/`-separated.
    pub local_prefix: String,
}

/// The worker's analysis of a built Lean project.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub success: Success,
    pub toolchain: lungo_protocol::WorkerToolchain,
    pub warnings: Vec<lungo_protocol::Diagnostic>,
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

/// Identifies everything a build's output depends on besides the input files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildKey {
    pub value: String,
}

fn env_string(var: &str) -> Result<String> {
    std::env::var(var).map_err(|_| {
        Error::Environment(format!("{var} is not set; Builder::compile_lean must run in a Cargo build script"))
    })
}

fn cargo_target() -> Result<Target> {
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

/// Stack reserved for the pipeline. Decoding, verifying and translating Bridge IR recurse over
/// the nesting of compiled code, which large programs make deep; the main thread's default
/// stack (1 MiB on Windows) is not enough for them. The memory is reserved, not committed.
const PIPELINE_STACK: usize = 512 << 20;

/// Runs `f` on a thread with [`PIPELINE_STACK`] of stack, forwarding a panic.
fn on_large_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .name("lungo".into())
            .stack_size(PIPELINE_STACK)
            .spawn_scoped(s, f)
            .expect("cannot start the lungo pipeline thread")
            .join()
            .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
    })
}

/// The canonical form of an existing path: absolute, with symbolic links resolved. On Windows
/// the path keeps its ordinary form (`C:\…`) rather than the verbatim form (`\\?\C:\…`)
/// wherever both denote the same file, so that it compares with paths from Cargo and Lake.
pub fn canonical_path(path: &Path) -> std::io::Result<PathBuf> {
    dunce::canonicalize(path)
}

/// `path` relative to `base`, `/`-separated, using `..` where needed. Never absolute.
pub fn relative_path(base: &Path, path: &Path) -> String {
    let base: Vec<Component> = base.components().collect();
    let path_c: Vec<Component> = path.components().collect();
    let common = base.iter().zip(&path_c).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base.len() {
        parts.push("..".into());
    }
    for c in &path_c[common..] {
        parts.push(c.as_os_str().to_string_lossy().into_owned());
    }
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_never_leak_absolute_prefixes() {
        assert_eq!(relative_path(Path::new("/a/b/crate"), Path::new("/a/b/crate/lean")), "lean");
        assert_eq!(relative_path(Path::new("/a/b/crate"), Path::new("/a/b/shared/lean")), "../shared/lean");
    }

    #[test]
    fn configuration_round_trips_through_toml() {
        let cfg = configure()
            .root_module("Formal")
            .export_module("Formal")
            .rust_extern("provider_send", "crate::provider::send")
            .type_attribute(".", "#[derive(serde::Serialize)]")
            .field_attribute("Formal.Sess.count", "#[serde(skip)]")
            .skip_debug(["Formal.Secret"])
            .deny_axioms(true);
        let text = toml::to_string(&cfg).unwrap();
        let back: Builder = toml::from_str(&text).unwrap();
        assert_eq!(cfg, back);
        assert!(toml::from_str::<Builder>("unknown = 1\n").is_err());
        assert_eq!(toml::from_str::<Builder>("").unwrap(), configure(), "every setting has a default");
        let file: ProjectFile = toml::from_str("project = \"lean\"\n[build]\nroot-modules = [\"A\"]\n").unwrap();
        assert_eq!(file.build, configure().root_module("A"));
        assert!(toml::from_str::<ProjectFile>("[build]\n").is_err(), "the project is required");
    }

    #[test]
    fn module_names_are_path_and_macro_safe() {
        for good in ["formal", "Formal_2", "type-error"] {
            assert!(validate_name(good, false).is_ok(), "{good}");
        }
        for bad in ["", "-x", "a/b", "a.b", "«x»", ".."] {
            assert!(matches!(validate_name(bad, false), Err(Error::Configuration(_))), "{bad}");
        }
    }
}
