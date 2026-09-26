//! Build-time integration of Lean projects into Rust crates.
//!
//! A Cargo build script describes a normal Lake project and the declarations to expose:
//!
//! ```no_run
//! use lean2rust_build::{Config, Mode};
//!
//! fn main() -> lean2rust_build::Result<()> {
//!     Config::new("lean")
//!         .root_module("Formal")
//!         .export_module("Formal")
//!         .mode(Mode::PureRust)
//!         .compile()
//! }
//! ```
//!
//! and the crate includes the generated code from `OUT_DIR`:
//!
//! ```ignore
//! pub mod formal {
//!     include!(concat!(env!("OUT_DIR"), "/lean2rust/formal.rs"));
//! }
//! ```
//!
//! Lake elaborates, kernel-checks, and compiles the Lean project with the exact toolchain it
//! pins; a worker compiled against that toolchain reads Lean's compiler output and hands it to
//! the Rust backend as Bridge IR. The `cargo lean2rust` command uses this same library.

mod error;
mod fingerprint;
mod lake;
mod oracle;
mod output;
mod toolchain;
mod worker;

pub use error::{Error, Result};
pub use toolchain::{SUPPORTED_TOOLCHAINS, Toolchain, ToolchainPolicy, read_pin};
pub use worker::{ADAPTER_VERSION, Limits, WorkerCache};

use fingerprint::Hasher;
use lean2rust_protocol::{
    CompilerOption, DiagnosticOptions, Endian, ExportPolicy, Outcome, PROTOCOL_VERSION, Request, Response, Success,
    Target,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// How the generated Rust executes the Lean program.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    /// Generated Rust on the lean2rust runtime; no Lean runtime or C code is linked.
    #[default]
    PureRust,
    /// The official Lean native backend and runtime, behind the same facade. Intended as a
    /// reference oracle for differential testing.
    LeanOracle,
}

/// Configuration of one Lean project's integration. Also loadable from a `lean2rust.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    /// The Lake project directory, relative to the Cargo package.
    pub project: PathBuf,
    #[serde(default)]
    pub root_modules: Vec<String>,
    /// Declarations that receive public facades.
    #[serde(default)]
    pub exports: Vec<String>,
    /// Modules (and their submodules) whose declarations receive public facades.
    #[serde(default)]
    pub export_modules: Vec<String>,
    #[serde(default)]
    pub mode: Mode,
    /// Application-provided implementations of extern symbols: symbol → Rust path.
    #[serde(default)]
    pub rust_externs: BTreeMap<String, String>,
    #[serde(default = "default_true")]
    pub deny_sorry: bool,
    #[serde(default)]
    pub deny_axioms: bool,
    #[serde(default)]
    pub deny_unsafe: bool,
    #[serde(default)]
    pub embed_sources: bool,
    /// The Lean namespace placed at the root of the generated module. Defaults to the first
    /// component of the first root module.
    #[serde(default)]
    pub facade_namespace: Option<String>,
    /// Stem of the aggregate include file. Defaults to the facade namespace in snake case.
    #[serde(default)]
    pub output_name: Option<String>,
    /// The directory under `OUT_DIR` receiving the generated files. Defaults to `lean2rust`;
    /// crates integrating several Lean projects give each its own directory.
    #[serde(default)]
    pub output_dir: Option<String>,
    /// Keep compiled workers in the build's output directory instead of the shared cache.
    #[serde(default)]
    pub hermetic_worker_cache: bool,
    /// Run the worker with a minimal environment.
    #[serde(default)]
    pub hermetic: bool,
    /// Seconds after which a running worker is terminated.
    #[serde(default)]
    pub worker_timeout: Option<u64>,
    /// Processor seconds the worker may use (see [`Limits`]).
    #[serde(default)]
    pub worker_cpu_limit: Option<u64>,
    /// Bytes of memory the worker may use (see [`Limits`]); an error on platforms that
    /// cannot enforce it.
    #[serde(default)]
    pub worker_memory_limit: Option<u64>,
    #[serde(default)]
    pub install_toolchain: bool,
    /// Use this toolchain installation instead of the one elan manages.
    #[serde(default)]
    pub toolchain_dir: Option<PathBuf>,
    /// Lean options in effect while the worker runs Lean metaprograms.
    #[serde(default)]
    pub lean_options: BTreeMap<String, String>,
    #[serde(default)]
    pub max_errors: u32,
}

fn default_true() -> bool {
    true
}

impl Config {
    pub fn new(project: impl Into<PathBuf>) -> Self {
        Config {
            project: project.into(),
            root_modules: Vec::new(),
            exports: Vec::new(),
            export_modules: Vec::new(),
            mode: Mode::PureRust,
            rust_externs: BTreeMap::new(),
            deny_sorry: true,
            deny_axioms: false,
            deny_unsafe: false,
            embed_sources: false,
            facade_namespace: None,
            output_name: None,
            output_dir: None,
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

    /// Loads a configuration from a TOML file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let text =
            std::fs::read_to_string(path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
        toml::from_str(&text).map_err(|e| Error::Project(format!("{}: {e}", path.display())))
    }

    pub fn root_module(mut self, module: impl Into<String>) -> Self {
        self.root_modules.push(module.into());
        self
    }

    pub fn export(mut self, declaration: impl Into<String>) -> Self {
        self.exports.push(declaration.into());
        self
    }

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

    pub fn output_name(mut self, name: impl Into<String>) -> Self {
        self.output_name = Some(name.into());
        self
    }

    /// Publishes the generated files into `$OUT_DIR/<dir>` instead of `$OUT_DIR/lean2rust`.
    pub fn output_dir(mut self, dir: impl Into<String>) -> Self {
        self.output_dir = Some(dir.into());
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

    pub fn lean_option(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.lean_options.insert(name.into(), value.into());
        self
    }

    /// Generates the Rust into `$OUT_DIR/lean2rust` and emits Cargo build-script directives.
    /// This is the entry point for `build.rs`.
    pub fn compile(self) -> Result<()> {
        let manifest_dir = env_path("CARGO_MANIFEST_DIR")?;
        let out_dir = env_path("OUT_DIR")?;
        let host = env_string("HOST")?;
        let target = cargo_target()?;
        let dir = self.output_dir.clone().unwrap_or_else(|| "lean2rust".to_owned());
        if dir.is_empty() || dir.contains(['/', '\\']) || dir == "." || dir == ".." {
            return Err(Error::Project(format!("output directory {dir:?} must be a single directory name")));
        }
        let env = Environment {
            manifest_dir,
            out_dir: out_dir.join(&dir),
            work_dir: out_dir.join(format!("{dir}-work")),
            host,
            target,
        };
        let outcome = self.run(&env)?;
        for input in &outcome.inputs {
            println!("cargo::rerun-if-changed={}", input.display());
        }
        for var in ["ELAN_HOME", "LEAN2RUST_CACHE_DIR"] {
            println!("cargo::rerun-if-env-changed={var}");
        }
        for link in &outcome.link_directives {
            println!("{link}");
        }
        Ok(())
    }

    /// Runs the complete pipeline for `env`, publishing the generated files into `env.out_dir`.
    pub fn run(&self, env: &Environment) -> Result<BuildOutcome> {
        let ctx = self.context(env)?;
        let key = self.build_key(&ctx, env)?;
        if let Some(previous) = output::read_build_info(&env.out_dir, &ctx.project)
            && previous.build_key == key.value
            && output::inputs_unchanged(&previous)
        {
            return Ok(BuildOutcome {
                inputs: previous.inputs.iter().map(PathBuf::from).collect(),
                link_directives: previous.link_directives.clone(),
                reused: true,
            });
        }
        let analysis = self.analyze_with(&ctx, env)?;
        let generated = self.generate_with(&ctx, env, &analysis)?;
        let inputs: Vec<PathBuf> = analysis.success.input_files.iter().map(|p| ctx.project.join(p)).collect();
        let info = output::BuildInfo::new(&key, &ctx, &analysis, &inputs, &generated.link_directives)?;
        output::publish(&env.out_dir, &env.work_dir, &generated.files, &generated.binary_files, &info)?;
        Ok(BuildOutcome { inputs, link_directives: generated.link_directives, reused: false })
    }

    /// Resolves the project, toolchain, and worker.
    pub fn context(&self, env: &Environment) -> Result<Context> {
        if self.root_modules.is_empty() {
            return Err(Error::Project("no root module configured; call Config::root_module".into()));
        }
        let project_rel = &self.project;
        let project_path =
            if project_rel.is_absolute() { project_rel.clone() } else { env.manifest_dir.join(project_rel) };
        let project = std::fs::canonicalize(&project_path)
            .map_err(|e| Error::io(format!("cannot resolve the Lean project {}", project_path.display()), e))?;
        let pin = toolchain::read_pin(&project)?;
        lake::validate_project(&project)?;
        let policy = if self.install_toolchain { ToolchainPolicy::Install } else { ToolchainPolicy::Strict };
        let toolchain = toolchain::resolve(&pin, self.toolchain_dir.as_deref(), policy)?;
        let cache = if self.hermetic_worker_cache {
            // `$OUT_DIR/lean2rust-toolchain`, beside (not inside) the published output.
            WorkerCache::Hermetic(env.out_dir.parent().unwrap_or(&env.out_dir).join("lean2rust-toolchain"))
        } else {
            WorkerCache::Shared(worker::default_shared_cache()?)
        };
        let worker_identity = worker::identity(&toolchain, &env.host);
        let local_prefix = relative_path(&env.manifest_dir, &project);
        Ok(Context { project, toolchain, cache, worker_identity, local_prefix })
    }

    /// Builds (or verifies) the worker for the project's toolchain.
    pub fn prepare_worker(&self, ctx: &Context, env: &Environment) -> Result<worker::Worker> {
        worker::prepare(&ctx.toolchain, &ctx.cache, &env.host)
    }

    fn request(&self, ctx: &Context, env: &Environment) -> Request {
        Request {
            protocol_version: PROTOCOL_VERSION,
            bridge_version: env!("CARGO_PKG_VERSION").into(),
            project_root: ctx.project.to_string_lossy().into_owned(),
            root_modules: self.root_modules.clone(),
            export_policy: ExportPolicy { declarations: self.exports.clone(), modules: self.export_modules.clone() },
            host_triple: env.host.clone(),
            target: env.target.clone(),
            compiler_options: self
                .lean_options
                .iter()
                .map(|(name, value)| CompilerOption { name: name.clone(), value: value.clone() })
                .collect(),
            hermetic: self.hermetic,
            diagnostics: DiagnosticOptions { max_errors: self.max_errors },
            runtime_exports: lean2rust_runtime::exports::REQUIRED.iter().map(|e| e.symbol.to_owned()).collect(),
        }
    }

    /// Builds the Lean project with Lake and runs the worker.
    pub fn analyze(&self, env: &Environment) -> Result<Analysis> {
        let ctx = self.context(env)?;
        self.analyze_with(&ctx, env)
    }

    fn analyze_with(&self, ctx: &Context, env: &Environment) -> Result<Analysis> {
        let worker = self.prepare_worker(ctx, env)?;
        lake::build(&ctx.toolchain, &ctx.project, &self.root_modules, true)?;
        let request = self.request(ctx, env);
        let response = worker::run(
            &worker,
            &ctx.toolchain,
            &request,
            &worker::RunOptions {
                project: &ctx.project,
                scratch: &env.work_dir,
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

    fn facade_namespace_for(&self) -> String {
        self.facade_namespace
            .clone()
            .unwrap_or_else(|| self.root_modules[0].split('.').next().expect("module names are non-empty").to_owned())
    }

    /// The stem of the aggregate include file.
    pub fn aggregate_name(&self) -> String {
        self.output_name.clone().unwrap_or_else(|| snake_case(&self.facade_namespace_for()))
    }

    /// Checks trust policies and generates the Rust files for an analysis.
    pub fn generate(&self, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        let ctx = self.context(env)?;
        self.generate_with(&ctx, env, analysis)
    }

    fn generate_with(&self, ctx: &Context, env: &Environment, analysis: &Analysis) -> Result<Generation> {
        self.check_trust(&analysis.success)?;
        let embedded =
            if self.embed_sources { Some(output::local_sources(&ctx.project, &analysis.success)?) } else { None };
        let namespace = self.facade_namespace_for();
        let aggregate = self.aggregate_name();
        match self.mode {
            Mode::PureRust => {
                let input = lean2rust_codegen::GenInput {
                    layer: lean2rust_codegen::Layer::PureRust,
                    success: &analysis.success,
                    toolchain: &analysis.toolchain,
                    facade_namespace: &namespace,
                    aggregate: &aggregate,
                    rust_externs: &self.rust_externs,
                    local_prefix: &ctx.local_prefix,
                    embedded_sources: embedded.as_ref(),
                };
                let generated = lean2rust_codegen::generate(&input).map_err(|errors| Error::Codegen {
                    toolchain: format!("v{}", analysis.toolchain.lean_version),
                    bir_version: analysis.success.bir.bir_version,
                    messages: errors.iter().map(|e| e.to_string()).collect(),
                })?;
                Ok(Generation { files: generated.files, binary_files: BTreeMap::new(), link_directives: Vec::new() })
            }
            Mode::LeanOracle => oracle::generate(self, ctx, env, analysis, &namespace, &aggregate, embedded.as_ref()),
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
            .field(&lean2rust_bir::BIR_VERSION.to_le_bytes())
            .field(&ADAPTER_VERSION.to_le_bytes())
            .str(env!("CARGO_PKG_VERSION"))
            .field(&lean2rust_runtime::ABI_VERSION.to_le_bytes())
            .str(&ctx.worker_identity)
            .str(&env.host)
            .str(&env.target.triple)
            .field(&env.target.pointer_width.to_le_bytes())
            .str(match env.target.endian {
                Endian::Little => "little",
                Endian::Big => "big",
            })
            .str(&ctx.local_prefix);
        for (path, digest) in lake::snapshot(&ctx.project)? {
            h.str(&relative_path(&ctx.project, &path)).str(&digest);
        }
        let config = serde_json::to_string(self).expect("configuration serializes");
        h.str(&config);
        // The generator itself: the running executable links lean2rust-build, the code
        // generator, and the runtime registry, so any change to them changes the key.
        let exe = std::env::current_exe().map_err(|e| Error::io("cannot locate the running generator", e))?;
        let exe_bytes = std::fs::read(&exe).map_err(|e| Error::io(format!("cannot read {}", exe.display()), e))?;
        h.str(&fingerprint::hash_bytes(&exe_bytes));
        Ok(BuildKey { value: h.finish() })
    }
}

/// Where and for what a build runs.
#[derive(Debug, Clone)]
pub struct Environment {
    /// The Cargo package directory; relative project paths are resolved against it.
    pub manifest_dir: PathBuf,
    /// The directory the generated files are published into.
    pub out_dir: PathBuf,
    /// Scratch space for worker requests and staged output.
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

/// The host triple lean2rust-build itself was compiled for.
pub fn host_triple() -> String {
    env!("LEAN2RUST_BUILD_HOST").to_owned()
}

/// The resolved project, toolchain, and worker cache.
#[derive(Debug, Clone)]
pub struct Context {
    pub project: PathBuf,
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
    pub toolchain: lean2rust_protocol::WorkerToolchain,
    pub warnings: Vec<lean2rust_protocol::Diagnostic>,
}

/// Generated files and the linker directives they need.
#[derive(Debug, Clone)]
pub struct Generation {
    pub files: BTreeMap<String, String>,
    /// Native artifacts (`LeanOracle` mode), keyed by path relative to the output directory.
    pub binary_files: BTreeMap<String, Vec<u8>>,
    pub link_directives: Vec<String>,
}

/// The result of [`Config::run`].
#[derive(Debug, Clone)]
pub struct BuildOutcome {
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

fn env_path(var: &str) -> Result<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).ok_or_else(|| {
        Error::Environment(format!("{var} is not set; Config::compile must run in a Cargo build script"))
    })
}

fn env_string(var: &str) -> Result<String> {
    std::env::var(var)
        .map_err(|_| Error::Environment(format!("{var} is not set; Config::compile must run in a Cargo build script")))
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

fn snake_case(s: &str) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if c.is_uppercase() {
            if i > 0 && !out.ends_with('_') {
                out.push('_');
            }
            out.extend(c.to_lowercase());
        } else if c.is_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_never_leak_absolute_prefixes() {
        assert_eq!(relative_path(Path::new("/a/b/crate"), Path::new("/a/b/crate/lean")), "lean");
        assert_eq!(relative_path(Path::new("/a/b/crate"), Path::new("/a/b/shared/lean")), "../shared/lean");
        assert_eq!(snake_case("Formal"), "formal");
        assert_eq!(snake_case("MyProject"), "my_project");
    }

    #[test]
    fn configuration_round_trips_through_toml() {
        let cfg = Config::new("lean")
            .root_module("Formal")
            .export_module("Formal")
            .rust_extern("provider_send", "crate::provider::send")
            .deny_axioms(true);
        let text = toml::to_string(&cfg).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(serde_json::to_string(&cfg).unwrap(), serde_json::to_string(&back).unwrap());
        assert!(toml::from_str::<Config>("project = \"lean\"\nunknown = 1\n").is_err());
    }
}
