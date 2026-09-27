//! `lungo generate`: every requested output from one analysis of the project per target.
//!
//! The Rust output is the crate module `build.rs` generates. Every other language is a
//! package generated from the program as C and its boundary: by a built-in generator, or by a
//! plugin. Each output directory belongs to lungo and is replaced as a whole, only when its
//! build key (the inputs, the toolchain, lungo, the generator and its options) changed.

use crate::runtime;
use lungo_build::codegen::c::{ProgramInput, generate_program};
use lungo_build::codegen::plugin::{ExternType, GenerateRequest, PROTOCOL_VERSION, ProgramInfo, RuntimeInfo, builtin};
use lungo_build::protocol::{Endian, Target};
use lungo_build::{Analysis, Builder, Context, Environment, Error, LeanOptions, Result, RustOptions};
use lungo_driver::output;
use serde::Serialize;
use std::cell::OnceCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One requested output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub language: String,
    pub dir: PathBuf,
    pub options: BTreeMap<String, String>,
    /// Lean types other generated packages provide (the language's `extern-types`).
    pub extern_types: BTreeMap<String, ExternType>,
}

pub struct Settings<'a> {
    pub project: &'a Path,
    pub lean: &'a LeanOptions,
    pub rust: &'a RustOptions,
    pub env: &'a Environment,
    pub rust_out: Option<PathBuf>,
    pub outputs: Vec<Output>,
    pub runtime_dir: Option<PathBuf>,
    pub wasi_sdk: Option<PathBuf>,
    /// Compare instead of writing (`--verify`).
    pub verify: bool,
}

/// The WebAssembly target of the TypeScript binding.
pub const WASM_TARGET: &str = "wasm32-wasip1";

/// The analysis of the project for each target, made on first use.
struct Analyses<'a> {
    lean: &'a LeanOptions,
    host: OnceCell<Analysis>,
    wasm: OnceCell<Analysis>,
}

impl Analyses<'_> {
    fn get(&self, ctx: &Context, env: &Environment) -> Result<&Analysis> {
        let cell = if env.target.triple == WASM_TARGET { &self.wasm } else { &self.host };
        if let Some(a) = cell.get() {
            return Ok(a);
        }
        let a = self.lean.analyze_in(ctx, env)?;
        self.lean.check_trust(&a.success)?;
        Ok(cell.get_or_init(|| a))
    }
}

fn wasm_env(env: &Environment) -> Environment {
    Environment {
        target: Target { triple: WASM_TARGET.to_owned(), pointer_width: 32, endian: Endian::Little },
        ..env.clone()
    }
}

/// What a binding output's build key covers besides the project: everything the generated
/// package depends on.
#[derive(Serialize)]
struct KeyConfig<'a> {
    language: &'a str,
    generator: &'a str,
    options: &'a BTreeMap<String, String>,
    extern_types: &'a BTreeMap<String, ExternType>,
    runtime: &'a RuntimeInfo,
    /// A local distribution's contents: it is rebuilt in place, and its path says nothing.
    distribution_digest: Option<&'a str>,
    host_externs: &'a std::collections::BTreeSet<String>,
    wasi_sdk: Option<&'a str>,
}

enum Generator {
    Builtin(&'static dyn lungo_build::codegen::plugin::Generator),
    /// A plugin program, identified by its digest in build keys.
    Plugin {
        program: PathBuf,
        digest: String,
    },
}

/// How the output directory `dir` differs from what the project generates now (`expected`).
fn drift(dir: &Path, expected: &BTreeMap<String, Vec<u8>>, actual: &BTreeMap<String, Vec<u8>>) -> Option<String> {
    let mut lines = Vec::new();
    for (rel, bytes) in expected {
        match actual.get(rel) {
            None => lines.push(format!("  missing: {rel}")),
            Some(b) if b != bytes => lines.push(format!("  changed: {rel}")),
            Some(_) => {}
        }
    }
    lines.extend(actual.keys().filter(|rel| !expected.contains_key(*rel)).map(|rel| format!("  extra:   {rel}")));
    (!lines.is_empty())
        .then(|| format!("{} is not what the project generates now:\n{}", dir.display(), lines.join("\n")))
}

/// Runs the generation, or with `verify` the comparison; one report line per output.
pub fn run(s: &Settings) -> Result<Vec<String>> {
    let analyses = Analyses { lean: s.lean, host: OnceCell::new(), wasm: OnceCell::new() };
    let mut report = Vec::new();
    let mut drifts = Vec::new();
    if let Some(dir) = &s.rust_out {
        let mut rust = s.rust.clone();
        rust.out_dir = None;
        let builder = Builder::from_options(s.lean.clone(), rust);
        if s.verify {
            // Generated beside the output and compared: the Rust module is the build's output.
            let scratch = dir.with_file_name(format!(
                ".lungo-verify-{}-{}",
                dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                std::process::id()
            ));
            let env = Environment { out_dir: scratch.clone(), ..s.env.clone() };
            let outcome = builder.run_with(s.project, &env, &|ctx| analyses.get(ctx, &env));
            let compared = outcome.and_then(|o| {
                let expected = output::output_files(&scratch.join(&o.name))?;
                let actual = output::output_files(&dir.join(&o.name))?;
                Ok((dir.join(&o.name), expected, actual))
            });
            if scratch.exists() {
                std::fs::remove_dir_all(&scratch)
                    .map_err(|e| Error::io(format!("cannot remove {}", scratch.display()), e))?;
            }
            let (at, expected, actual) = compared?;
            match drift(&at, &expected, &actual) {
                Some(d) => drifts.push(d),
                None => report.push(format!("verified: rust {}", at.display())),
            }
        } else {
            let env = Environment { out_dir: dir.clone(), ..s.env.clone() };
            let outcome = builder.run_with(s.project, &env, &|ctx| analyses.get(ctx, &env))?;
            let at = dir.join(&outcome.name);
            report.push(if outcome.reused {
                format!("up to date: rust {}", at.display())
            } else {
                format!("generated rust into {}", at.display())
            });
        }
    }
    if s.outputs.is_empty() {
        return finish(report, drifts);
    }
    // Every generator is resolved before anything is generated.
    let mut generators = Vec::new();
    for o in &s.outputs {
        generators.push(match builtin(&o.language) {
            Some(g) => Generator::Builtin(g),
            None => {
                let program = crate::plugin::find(&o.language)?;
                let digest = runtime::file_sha256(&program)?;
                Generator::Plugin { program, digest }
            }
        });
    }
    let runtime = runtime::select(s.runtime_dir.as_deref())?;
    let distribution_digest = runtime::distribution_digest(&runtime)?;
    let ctx = s.lean.context(s.project, s.env)?;
    let wasm = wasm_env(s.env);
    for (o, generator) in s.outputs.iter().zip(&generators) {
        let env = if o.language == "ts" { &wasm } else { s.env };
        let identity = match generator {
            Generator::Builtin(_) => format!("builtin {}", lungo_build::codegen::GENERATOR_VERSION),
            Generator::Plugin { program, digest } => format!("plugin {} {digest}", program.display()),
        };
        let sdk = if o.language == "ts" { Some(crate::wasm::wasi_sdk(s.wasi_sdk.as_deref())?) } else { None };
        let sdk_text = sdk.as_ref().map(|p| p.to_string_lossy().into_owned());
        let config = serde_json::to_string(&KeyConfig {
            language: &o.language,
            generator: &identity,
            options: &o.options,
            extern_types: &o.extern_types,
            runtime: &runtime,
            distribution_digest: distribution_digest.as_deref(),
            host_externs: &s.lean.host_externs,
            wasi_sdk: sdk_text.as_deref(),
        })
        .expect("key configuration serializes");
        let key = lungo_driver::build_key(&ctx, env, &config)?;
        if !s.verify
            && let Some(previous) = output::read_build_info(&o.dir, &ctx.project)
            && previous.build_key == key.value
            && output::still_current(&o.dir, &previous)
        {
            report.push(format!("up to date: {} {}", o.language, o.dir.display()));
            continue;
        }
        let analysis = analyses.get(&ctx, env)?;
        let dir =
            std::path::absolute(&o.dir).map_err(|e| Error::io(format!("cannot resolve {}", o.dir.display()), e))?;
        let local_prefix = lungo_build::relative_path(&dir, &ctx.project);
        let program = generate_program(&ProgramInput {
            success: &analysis.success,
            toolchain: &analysis.toolchain,
            name: &ctx.name,
            host_externs: &s.lean.host_externs,
            local_prefix: &local_prefix,
            target: &env.target.triple,
        })
        .map_err(|errors| codegen_error(analysis, errors))?;
        let request = GenerateRequest {
            protocol_version: PROTOCOL_VERSION,
            program: ProgramInfo {
                name: ctx.name.clone(),
                lean_version: analysis.toolchain.lean_version.clone(),
                lean_githash: analysis.toolchain.lean_githash.clone(),
                bir_version: analysis.success.bir.bir_version,
                root_modules: analysis.success.root_modules.clone(),
            },
            boundary: program.boundary.clone(),
            program_files: program.files.clone(),
            runtime: runtime.clone(),
            options: o.options.clone(),
            extern_types: o.extern_types.clone(),
        };
        let files = match generator {
            Generator::Builtin(g) => g.generate(&request).map_err(|errors| codegen_error(analysis, errors))?,
            Generator::Plugin { program, .. } => crate::plugin::run(program, &request)?,
        };
        let work = dir.parent().expect("an absolute path has a parent").join(format!(
            ".lungo-work-{}",
            dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
        ));
        let mut binary = BTreeMap::new();
        if let Some(sdk) = &sdk {
            let runtime_package = runtime::runtime_package(&runtime, WASM_TARGET)?;
            let module = crate::wasm::link(&program.files, &program.boundary, &runtime_package, sdk, &work)?;
            binary.insert("program.wasm".to_owned(), module);
        }
        if s.verify {
            if work.exists() {
                std::fs::remove_dir_all(&work)
                    .map_err(|e| Error::io(format!("cannot remove {}", work.display()), e))?;
            }
            let expected: BTreeMap<String, Vec<u8>> = files
                .iter()
                .map(|(k, v)| (k.clone(), v.as_bytes().to_vec()))
                .chain(binary.iter().map(|(k, v)| (k.clone(), v.clone())))
                .collect();
            match drift(&o.dir, &expected, &output::output_files(&dir)?) {
                Some(d) => drifts.push(d),
                None => report.push(format!("verified: {} {}", o.language, o.dir.display())),
            }
            continue;
        }
        let inputs: Vec<PathBuf> = analysis.success.input_files.iter().map(|p| ctx.project.join(p)).collect();
        let info = output::BuildInfo::new(&key, &ctx, analysis, &inputs, &[])?;
        output::publish(&dir, &work, &files, &binary, &info)?;
        std::fs::remove_dir_all(&work).map_err(|e| Error::io(format!("cannot remove {}", work.display()), e))?;
        report.push(format!("generated {} into {}", o.language, o.dir.display()));
    }
    finish(report, drifts)
}

/// The report, or every output that is not what the project generates now.
fn finish(report: Vec<String>, drifts: Vec<String>) -> Result<Vec<String>> {
    if drifts.is_empty() {
        Ok(report)
    } else {
        Err(Error::OutputDrift(format!("{}\nRun `lungo generate` and commit what it writes.", drifts.join("\n"))))
    }
}

fn codegen_error(analysis: &Analysis, errors: Vec<lungo_build::CodegenError>) -> Error {
    Error::Codegen {
        toolchain: format!("v{}", analysis.toolchain.lean_version),
        bir_version: analysis.success.bir.bir_version,
        errors,
    }
}
