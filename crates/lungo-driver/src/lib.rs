//! The language-neutral lungo pipeline.
//!
//! Lake elaborates, kernel-checks and compiles a Lake project with the exact toolchain it pins;
//! a worker compiled against that toolchain reads Lean's compiler output and reports the program
//! as Bridge IR with its interface ([`Analysis`]). The code generators turn an analysis into Rust
//! (`lungo-build`, for Cargo build scripts) or into C and a binding in another language (the
//! `lungo` command). [`LeanOptions`] are the settings every language shares: what is compiled
//! and exported, the trust policy, and how the worker runs.

mod error;
pub mod fingerprint;
pub mod lake;
pub mod output;
mod toolchain;
mod worker;

pub use error::{Error, Result};
pub use lungo_codegen::{CodegenError, ErrorCode};
pub use toolchain::{SUPPORTED_TOOLCHAINS, Toolchain, ToolchainPolicy, read_pin};
pub use worker::{ADAPTER_VERSION, Limits, Worker, WorkerCache};

// The crates whose types appear in this crate's API, for tools built on the pipeline: through
// these, they use exactly the versions this crate was built with.
/// The Bridge IR: the compiled Lean program the worker reports, and its verifier.
pub use lungo_bir as bir;
/// The code generators, and the conventions of the files they generate.
pub use lungo_codegen as codegen;
/// The protocol between this crate and its Lean worker, including the worker's analysis.
pub use lungo_protocol as protocol;

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

/// The settings every generated language shares: what is compiled and exported, the externs the
/// application implements, the trust policy, and how the worker runs. Loadable from the `[lean]`
/// table of a `lungo.toml`; the Rust `Builder` of `lungo-build` includes them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct LeanOptions {
    /// Modules whose code is compiled. Empty: the root modules of the Lake package's default
    /// targets, as Lake resolves them.
    pub root_modules: Vec<String>,
    /// Declarations that receive public functions.
    pub exports: Vec<String>,
    /// Modules (and their submodules) whose declarations receive public functions. When neither
    /// this nor `exports` names anything, the root modules are exported.
    pub export_modules: Vec<String>,
    /// Extern keys (symbols, or declaration names for other extern forms) the application
    /// implements in the host language.
    pub host_externs: BTreeSet<String>,
    /// The name of the generated program (module, package, symbol prefix). Defaults to the
    /// Lake package's name.
    pub name: Option<String>,
    pub deny_sorry: bool,
    pub deny_axioms: bool,
    pub deny_unsafe: bool,
    /// Keep compiled workers in the build's work directory instead of the shared cache.
    pub hermetic_worker_cache: bool,
    /// Run the worker with a minimal environment.
    pub hermetic: bool,
    /// Seconds after which a running worker is terminated.
    pub worker_timeout: Option<u64>,
    /// Processor seconds the worker may use (see [`Limits`]).
    pub worker_cpu_limit: Option<u64>,
    /// Bytes of memory the worker may use (see [`Limits`]); an error on platforms that cannot
    /// enforce it.
    pub worker_memory_limit: Option<u64>,
    pub install_toolchain: bool,
    /// Use this toolchain installation instead of the one elan manages.
    pub toolchain_dir: Option<PathBuf>,
    /// Lean options in effect while the worker runs Lean metaprograms.
    pub lean_options: BTreeMap<String, String>,
    /// Report at most this many errors from the worker; `0` reports all.
    pub max_errors: u32,
}

impl Default for LeanOptions {
    fn default() -> Self {
        LeanOptions {
            root_modules: Vec::new(),
            exports: Vec::new(),
            export_modules: Vec::new(),
            host_externs: BTreeSet::new(),
            name: None,
            deny_sorry: true,
            deny_axioms: false,
            deny_unsafe: false,
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

impl LeanOptions {
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

    /// Declares that the application implements the Lean extern `key` in the host language.
    pub fn host_extern(mut self, key: impl Into<String>) -> Self {
        self.host_externs.insert(key.into());
        self
    }

    /// Names the generated program instead of the Lake package's name. Letters, digits, `_`
    /// and `-`.
    pub fn name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
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

    /// Resolves the project, the program's name and output, the toolchain, and the worker.
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
        Ok(Context { project, out_dir: env.out_dir.join(&name), work_dir, name, toolchain, cache, worker_identity, local_prefix })
    }

    /// Builds (or verifies) the worker for the project's toolchain.
    pub fn prepare_worker(&self, ctx: &Context, env: &Environment) -> Result<Worker> {
        worker::prepare(&ctx.toolchain, &ctx.cache, &env.host)
    }

    fn roots(&self) -> Roots {
        if self.root_modules.is_empty() { Roots::DefaultTargets } else { Roots::Modules(self.root_modules.clone()) }
    }

    pub(crate) fn request(&self, ctx: &Context, env: &Environment) -> Request {
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
            self.analyze_in(&ctx, env)
        })
    }

    /// Builds the resolved project with Lake and runs the worker.
    pub fn analyze_in(&self, ctx: &Context, env: &Environment) -> Result<Analysis> {
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

    /// Checks the trust policy against every export of the program.
    pub fn check_trust(&self, success: &Success) -> Result<()> {
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
}

/// Identifies everything a build's output depends on besides the input files: the toolchain,
/// the versions of lungo and its worker, the target, the project's Lake configuration,
/// `config` (the serialized configuration of the generator), and the running generator itself.
pub fn build_key(ctx: &Context, env: &Environment, config: &str) -> Result<BuildKey> {
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
    h.str(config);
    // The generator itself: the running executable links the pipeline, the code generators and
    // the runtime registry, so any change to them changes the key.
    let exe = std::env::current_exe().map_err(|e| Error::io("cannot locate the running generator", e))?;
    let exe_bytes = std::fs::read(&exe).map_err(|e| Error::io(format!("cannot read {}", exe.display()), e))?;
    h.str(&fingerprint::hash_bytes(&exe_bytes));
    Ok(BuildKey { value: h.finish() })
}

/// Checks that `name` can name the generated program: a directory and file name, a module or
/// package name after case conversion, and a C symbol prefix after replacing `-`.
fn validate_name(name: &str, from_package: bool) -> Result<()> {
    let valid = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        && !name.starts_with(|c: char| c == '-' || c.is_ascii_digit());
    if valid {
        return Ok(());
    }
    Err(Error::Configuration(if from_package {
        format!(
            "the Lake package name {name:?} cannot name the generated program (letters, digits, `_` and `-`, not starting with a digit or `-`); choose one with the `name` setting"
        )
    } else {
        format!("the program name {name:?} is invalid: use letters, digits, `_` and `-`, not starting with a digit or `-`")
    }))
}

/// Output directories written by this process, and the projects that wrote them: two builds in
/// one process must not publish into the same directory.
static CLAIMED_OUTPUTS: Mutex<BTreeMap<PathBuf, PathBuf>> = Mutex::new(BTreeMap::new());

/// Claims the context's output directory for its project.
pub fn claim_output(ctx: &Context) -> Result<()> {
    let mut claimed = CLAIMED_OUTPUTS.lock().unwrap_or_else(|p| p.into_inner());
    match claimed.get(&ctx.out_dir) {
        Some(owner) if owner != &ctx.project => Err(Error::Configuration(format!(
            "the Lean projects {} and {} would both generate the program `{}`; give one another name with the `name` setting",
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
    /// The package directory; relative project paths are resolved against it.
    pub manifest_dir: PathBuf,
    /// The directory generated programs are published into, each as `<out_dir>/<name>`.
    pub out_dir: PathBuf,
    /// Scratch space for worker requests and staged output, per program in `<work_dir>/<name>`.
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

/// The C ABI version of the runtime this pipeline generates code for (`LUNGO_ABI_VERSION`).
pub const RUNTIME_ABI_VERSION: u32 = lungo_runtime::ABI_VERSION;

/// The root of lungo's cache, shared by every build of the user: `LUNGO_CACHE_DIR`, else the
/// platform's cache directory. It holds compiled workers and downloaded runtimes.
pub fn cache_root() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("LUNGO_CACHE_DIR") {
        return Ok(PathBuf::from(dir));
    }
    if cfg!(windows) {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(dir).join("lungo"));
        }
    } else if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(dir).join("lungo"));
    } else if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".cache").join("lungo"));
    }
    Err(Error::Environment(
        "cannot determine lungo's cache directory; set LUNGO_CACHE_DIR (or use a hermetic worker cache)".into(),
    ))
}

/// The host triple the pipeline was compiled for.
pub fn host_triple() -> String {
    env!("LUNGO_DRIVER_HOST").to_owned()
}

/// The resolved project, program, toolchain, and worker cache.
#[derive(Debug, Clone)]
pub struct Context {
    pub project: PathBuf,
    /// The program's name.
    pub name: String,
    /// Where the program is published: `<out_dir>/<name>`.
    pub out_dir: PathBuf,
    /// Scratch space for this program.
    pub work_dir: PathBuf,
    pub toolchain: Toolchain,
    pub cache: WorkerCache,
    pub worker_identity: String,
    /// The project directory relative to the package, `/`-separated.
    pub local_prefix: String,
}

/// The worker's analysis of a built Lean project.
#[derive(Debug, Clone)]
pub struct Analysis {
    pub success: Success,
    pub toolchain: lungo_protocol::WorkerToolchain,
    pub warnings: Vec<lungo_protocol::Diagnostic>,
}

/// Identifies everything a build's output depends on besides the input files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildKey {
    pub value: String,
}

/// Stack reserved for the pipeline. Decoding, verifying and translating Bridge IR recurse over
/// the nesting of compiled code, which large programs make deep; the main thread's default
/// stack (1 MiB on Windows) is not enough for them. The memory is reserved, not committed.
const PIPELINE_STACK: usize = 512 << 20;

/// Runs `f` on a thread with enough stack for the pipeline, forwarding a panic.
pub fn on_large_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
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
    fn options_round_trip_through_toml_and_reject_unknown_keys() {
        let options = LeanOptions::default().root_module("Formal").host_extern("host_log").deny_axioms(true);
        let text = toml::to_string(&options).unwrap();
        assert_eq!(toml::from_str::<LeanOptions>(&text).unwrap(), options);
        assert!(toml::from_str::<LeanOptions>("unknown = 1\n").is_err());
        assert_eq!(toml::from_str::<LeanOptions>("").unwrap(), LeanOptions::default(), "every setting has a default");
    }

    #[test]
    fn program_names_are_path_macro_and_symbol_safe() {
        for good in ["formal", "Formal_2", "type-error"] {
            assert!(validate_name(good, false).is_ok(), "{good}");
        }
        for bad in ["", "-x", "2d", "a/b", "a.b", "«x»", ".."] {
            assert!(matches!(validate_name(bad, false), Err(Error::Configuration(_))), "{bad}");
        }
    }
}
