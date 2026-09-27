//! `lungo`: generate code in many languages from a Lean program, as `protoc` does from `.proto`
//! files.
//!
//! ```text
//! lungo generate --c_out=gen/c --go_out=gen/go --python_out=gen/py --rust_out=gen/rust
//! ```
//!
//! Every language runs the program on the same runtime: Rust links it as a crate; the others
//! link the program as C against the prebuilt runtime library, which the generated packages
//! download from the lungo release (verified by SHA-256). The configuration is `lungo.toml`
//! (see `config`); command-line options add to it.

mod config;
mod generate;
mod plugin;
mod runtime;
mod wasm;

use clap::{Args, Parser, Subcommand};
use config::{ProjectFile, validate_language_name};
use generate::{Output, Settings};
use lungo_build::{Analysis, Builder, Environment, Error, LeanOptions, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "lungo", version, about = "Generate C, Go, Python, Swift, TypeScript and Rust from Lean programs")]
struct Cli {
    #[command(flatten)]
    options: Options,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone, Default)]
struct Options {
    /// The configuration file (default: `lungo.toml` in the current directory).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// The Lake project directory, instead of a configuration file.
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// A root module (repeatable; default: the Lake package's default targets).
    #[arg(long = "root", global = true)]
    roots: Vec<String>,
    /// A declaration to export (repeatable).
    #[arg(long = "export", global = true)]
    exports: Vec<String>,
    /// A module whose declarations to export (repeatable).
    #[arg(long = "export-module", global = true)]
    export_modules: Vec<String>,
    /// An extern the application implements in the host language (repeatable).
    #[arg(long = "host-extern", global = true)]
    host_externs: Vec<String>,
    /// The program's name (default: the Lake package's).
    #[arg(long, global = true)]
    name: Option<String>,
}

#[derive(Subcommand)]
enum Command {
    /// Generate outputs: `--<language>_out=DIR` for rust, c, go, python, swift, ts or a plugin
    /// `lungo-gen-<language>`, with `--<language>_opt=KEY=VALUE`; without any, the outputs
    /// `lungo.toml` configures.
    Generate(GenerateArgs),
    /// Build the Lean project and check that it translates, without writing output.
    Check,
    /// Show what lungo knows about a declaration.
    Inspect { declaration: String },
    /// Print the Bridge IR of a declaration and the compiler auxiliaries derived from it.
    Ir { declaration: String },
    /// Print the generated Rust of a declaration.
    Rust { declaration: String },
    /// List every extern the program reaches and how it is implemented.
    Externs,
    /// List the Lean-name to Rust-path mappings of the Rust output.
    Mappings,
    /// Build (or verify) the Lean worker for the project's toolchain.
    Prepare,
    /// Explicitly install the pinned toolchain and materialize locked Lake dependencies.
    Setup,
    /// The prebuilt lungo runtime of this release.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
}

#[derive(Args)]
struct GenerateArgs {
    /// A local lungo distribution to use instead of this release's (required by a development
    /// build of lungo for every language but Rust).
    #[arg(long)]
    runtime_dir: Option<PathBuf>,
    /// The wasi-sdk to link the TypeScript binding's WebAssembly with (default: `WASI_SDK_PATH`,
    /// else the pinned release, downloaded and verified).
    #[arg(long)]
    wasi_sdk: Option<PathBuf>,
    /// Write nothing: check that every output directory holds exactly what the project
    /// generates now, and fail with LNG0110 naming each file that differs, is missing or is extra.
    #[arg(long)]
    verify: bool,
}

#[derive(Subcommand)]
enum RuntimeCommand {
    /// Download the runtime archive for a target into the cache, verifying its SHA-256.
    Fetch(TargetArg),
    /// Print the directory of the cached runtime for a target.
    Path(TargetArg),
    /// Download the runtime archive for a target again and check its SHA-256.
    Verify(TargetArg),
}

#[derive(Args)]
struct TargetArg {
    /// The target triple (default: this machine's).
    #[arg(long)]
    target: Option<String>,
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (args, flags) = match split_language_flags(args) {
        Ok(split) => split,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let cli = Cli::parse_from(args);
    match run(cli, flags) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

/// `--<language>_out` and `--<language>_opt` flags, which name languages clap cannot know.
#[derive(Debug, Default, PartialEq, Eq)]
struct LanguageFlags {
    outs: Vec<(String, PathBuf)>,
    opts: Vec<(String, String, String)>,
}

/// Separates the language flags (`--X_out=DIR`, `--X_out DIR`, `--X_opt=K=V[,K=V…]`) from the
/// other arguments.
fn split_language_flags(args: Vec<String>) -> Result<(Vec<String>, LanguageFlags)> {
    let mut rest = Vec::new();
    let mut flags = LanguageFlags::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        let Some(body) = arg.strip_prefix("--") else {
            rest.push(arg);
            continue;
        };
        let (name, inline) = match body.split_once('=') {
            Some((n, v)) => (n.to_owned(), Some(v.to_owned())),
            None => (body.to_owned(), None),
        };
        let (language, kind) = match (name.strip_suffix("_out"), name.strip_suffix("_opt")) {
            (Some(l), _) => (l.to_owned(), "out"),
            (_, Some(l)) => (l.to_owned(), "opt"),
            _ => {
                rest.push(arg);
                continue;
            }
        };
        let value = match inline {
            Some(v) => v,
            None => it.next().ok_or_else(|| Error::Configuration(format!("--{name} needs a value")))?,
        };
        if language != "rust" {
            validate_language_name(&language)?;
        }
        if kind == "out" {
            flags.outs.push((language, PathBuf::from(value)));
        } else {
            for pair in value.split(',') {
                let (k, v) = pair
                    .split_once('=')
                    .ok_or_else(|| Error::Configuration(format!("--{name} takes KEY=VALUE pairs, not `{pair}`")))?;
                flags.opts.push((language.clone(), k.to_owned(), v.to_owned()));
            }
        }
    }
    Ok((rest, flags))
}

/// The configuration: `--config`, else `lungo.toml` in the current directory, else `--project`
/// alone; with the command-line options added. Relative paths are relative to the configuration
/// file's directory (the current directory with `--project`).
fn load(opts: &Options) -> Result<(PathBuf, ProjectFile)> {
    let cwd = std::env::current_dir().map_err(|e| Error::io("cannot determine the current directory", e))?;
    let file = opts.config.clone().or_else(|| {
        let default = cwd.join("lungo.toml");
        default.is_file().then_some(default)
    });
    let (base, mut project) = match (&file, &opts.project) {
        (Some(path), None) => {
            let path = lungo_build::canonical_path(path)
                .map_err(|e| Error::io(format!("cannot resolve {}", path.display()), e))?;
            let base = path.parent().expect("a file has a parent").to_path_buf();
            (base, ProjectFile::load(&path)?)
        }
        (None, Some(project)) => (cwd, ProjectFile { project: project.clone(), ..ProjectFile::default() }),
        (Some(_), Some(_)) => {
            return Err(Error::Configuration("give either a configuration file or --project, not both".into()));
        }
        (None, None) => {
            return Err(Error::Configuration(format!(
                "no lungo configuration: create {} or pass --project",
                cwd.join("lungo.toml").display()
            )));
        }
    };
    let lean = &mut project.lean;
    lean.root_modules.extend(opts.roots.iter().cloned());
    lean.exports.extend(opts.exports.iter().cloned());
    lean.export_modules.extend(opts.export_modules.iter().cloned());
    lean.host_externs.extend(opts.host_externs.iter().cloned());
    if let Some(name) = &opts.name {
        lean.name = Some(name.clone());
    }
    Ok((base, project))
}

/// The environment of the commands: lungo's scratch space is `.lungo/` next to the
/// configuration.
fn environment(base: &Path) -> Environment {
    let work = base.join(".lungo");
    Environment::native(base.to_path_buf(), work.join("out"), work.join("work"))
}

fn run(cli: Cli, flags: LanguageFlags) -> Result<()> {
    let generating = matches!(cli.command, Command::Generate(_));
    if !generating && flags != LanguageFlags::default() {
        return Err(Error::Configuration(
            "--<language>_out and --<language>_opt are options of `lungo generate`".into(),
        ));
    }
    if let Command::Runtime { command } = &cli.command {
        return runtime_command(command);
    }
    let (base, file) = load(&cli.options)?;
    let env = environment(&base);
    let project = file.project.clone();
    let builder = || Builder::from_options(file.lean.clone(), file.rust.clone());
    match cli.command {
        Command::Generate(args) => {
            let settings = generate_settings(&base, &file, &env, flags, args)?;
            let report = lungo_driver::on_large_stack(|| generate::run(&settings))?;
            for line in report {
                println!("{line}");
            }
        }
        Command::Check => {
            let (analysis, files) = lungo_driver::on_large_stack(|| check(&file.lean, &builder(), &project, &env))?;
            report_summary(&analysis);
            println!("generation succeeded: {files} files");
        }
        Command::Inspect { declaration } => {
            let ctx = file.lean.context(&project, &env)?;
            inspect(&file.lean.analyze(&project, &env)?, &declaration, &ctx.local_prefix)?
        }
        Command::Ir { declaration } => ir(&file.lean.analyze(&project, &env)?, &declaration)?,
        Command::Rust { declaration } => {
            let b = builder();
            let ctx = b.context(&project, &env)?;
            let analysis = b.analyze(&project, &env)?;
            let generation = b.generate(&project, &env, &analysis)?;
            rust(&analysis, &generation, &declaration, &ctx.name)?;
        }
        Command::Externs => {
            let b = builder();
            let generation = b.generate(&project, &env, &b.analyze(&project, &env)?)?;
            print!("{}", generation.files.get("externs.json").ok_or(Error::Configuration("no extern report".into()))?);
        }
        Command::Mappings => {
            let b = builder();
            let generation = b.generate(&project, &env, &b.analyze(&project, &env)?)?;
            print!("{}", generation.files.get("names.json").ok_or(Error::Configuration("no name mappings".into()))?);
        }
        Command::Prepare => {
            let ctx = file.lean.context(&project, &env)?;
            let worker = file.lean.prepare_worker(&ctx, &env)?;
            println!("Lean {} ({})", ctx.toolchain.version, ctx.toolchain.githash);
            println!("worker {} at {}", worker.identity, worker.binary.display());
        }
        Command::Setup => setup(&project, file.lean.clone(), &base, &env)?,
        Command::Runtime { .. } => unreachable!("handled above"),
    }
    Ok(())
}

/// The outputs to generate: the language flags, else the outputs the configuration names.
fn generate_settings<'a>(
    base: &Path,
    file: &'a ProjectFile,
    env: &'a Environment,
    flags: LanguageFlags,
    args: GenerateArgs,
) -> Result<Settings<'a>> {
    // Paths of the configuration are relative to its file, paths of the command line to the
    // current directory.
    let resolve = |p: &Path| if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let cwd = std::env::current_dir().map_err(|e| Error::io("cannot determine the current directory", e))?;
    let resolve_arg = |p: &Path| cwd.join(p);
    let mut rust_out = None;
    let mut outputs: Vec<Output> = Vec::new();
    let mut cli_options: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    for (language, key, value) in flags.opts {
        if language == "rust" {
            return Err(Error::Configuration(
                "the Rust generator's settings are typed: set them in the [rust] table of lungo.toml".into(),
            ));
        }
        cli_options.entry(language).or_default().insert(key, value);
    }
    let options = |language: &str| -> BTreeMap<String, String> {
        let mut o = file.language(language).map(|l| l.options.clone()).unwrap_or_default();
        o.extend(cli_options.get(language).cloned().unwrap_or_default());
        o
    };
    let extern_types = |language: &str| file.language(language).map(|l| l.extern_types.clone()).unwrap_or_default();
    if flags.outs.is_empty() {
        rust_out = file.rust.out_dir.as_deref().map(resolve);
        for (language, settings) in file.languages() {
            if let Some(out) = &settings.out {
                outputs.push(Output {
                    dir: resolve(out),
                    options: options(&language),
                    extern_types: extern_types(&language),
                    language,
                });
            }
        }
        if rust_out.is_none() && outputs.is_empty() {
            return Err(Error::Configuration(
                "nothing to generate: pass --<language>_out=DIR, or set `out` of a language in lungo.toml".into(),
            ));
        }
    } else {
        for (language, dir) in flags.outs {
            if language == "rust" {
                if rust_out.replace(resolve_arg(&dir)).is_some() {
                    return Err(Error::Configuration("--rust_out is given twice".into()));
                }
                continue;
            }
            if outputs.iter().any(|o| o.language == language) {
                return Err(Error::Configuration(format!("--{language}_out is given twice")));
            }
            outputs.push(Output {
                dir: resolve_arg(&dir),
                options: options(&language),
                extern_types: extern_types(&language),
                language,
            });
        }
    }
    for language in cli_options.keys() {
        if !outputs.iter().any(|o| &o.language == language) {
            return Err(Error::Configuration(format!("--{language}_opt is given without an output for {language}")));
        }
    }
    let mut dirs: Vec<&PathBuf> = outputs.iter().map(|o| &o.dir).chain(rust_out.iter()).collect();
    dirs.sort();
    if let Some(w) = dirs.windows(2).find(|w| w[0] == w[1] || w[1].starts_with(w[0])) {
        return Err(Error::Configuration(format!(
            "outputs must be in separate directories: {} and {} overlap",
            w[0].display(),
            w[1].display()
        )));
    }
    Ok(Settings {
        project: &file.project,
        lean: &file.lean,
        rust: &file.rust,
        env,
        rust_out,
        outputs,
        runtime_dir: args.runtime_dir,
        wasi_sdk: args.wasi_sdk,
        verify: args.verify,
    })
}

fn runtime_command(command: &RuntimeCommand) -> Result<()> {
    let manifest = runtime::embedded_manifest()?.ok_or_else(|| {
        Error::RuntimeUnavailable("this lungo is a development build, which knows no runtime release".into())
    })?;
    let target = |t: &TargetArg| t.target.clone().unwrap_or_else(lungo_build::host_triple);
    let dir = match command {
        RuntimeCommand::Fetch(t) => runtime::fetch(&manifest.artifacts, &target(t))?,
        RuntimeCommand::Path(t) => runtime::cached(&manifest.artifacts, &target(t))?,
        RuntimeCommand::Verify(t) => runtime::verify(&manifest.artifacts, &target(t))?,
    };
    println!("{}", dir.display());
    Ok(())
}

/// Analyzes the project and generates every output in memory: the Rust module and the program
/// as C.
fn check(lean: &LeanOptions, builder: &Builder, project: &Path, env: &Environment) -> Result<(Analysis, usize)> {
    let ctx = lean.context(project, env)?;
    let analysis = lean.analyze_in(&ctx, env)?;
    let generation = builder.generate(project, env, &analysis)?;
    let program = lungo_build::codegen::c::generate_program(&lungo_build::codegen::c::ProgramInput {
        success: &analysis.success,
        toolchain: &analysis.toolchain,
        name: &ctx.name,
        host_externs: &lean.host_externs,
        local_prefix: &ctx.local_prefix,
        target: &env.target.triple,
    })
    .map_err(|errors| Error::Codegen {
        toolchain: format!("v{}", analysis.toolchain.lean_version),
        bir_version: analysis.success.bir.bir_version,
        errors,
    })?;
    let files = generation.files.len() + generation.binary_files.len() + program.files.len();
    Ok((analysis, files))
}

fn setup(project: &Path, lean: LeanOptions, base: &Path, env: &Environment) -> Result<()> {
    let lean = lean.toolchain_policy(lungo_build::ToolchainPolicy::Install);
    let project = if project.is_absolute() { project.to_path_buf() } else { base.join(project) };
    let pin = lungo_build::read_pin(&project)?;
    let toolchain = lungo_build::Toolchain::resolve_pin(
        &pin,
        lean.toolchain_dir.as_deref(),
        lungo_build::ToolchainPolicy::Install,
    )?;
    // Loading the workspace materializes the dependencies locked in the manifest.
    let status = std::process::Command::new(&toolchain.lake)
        .args(["env", "lean", "--version"])
        .current_dir(&project)
        .status()
        .map_err(|e| Error::io("cannot run lake", e))?;
    if !status.success() {
        return Err(Error::Command {
            program: "lake env lean --version".into(),
            status: status.to_string(),
            output: "materializing the Lake dependencies failed".into(),
        });
    }
    let ctx = lean.context(&project, env)?;
    let worker = lean.prepare_worker(&ctx, env)?;
    println!("toolchain {} ready; worker {}", ctx.toolchain.pin, worker.identity);
    Ok(())
}

fn report_summary(analysis: &Analysis) {
    let s = &analysis.success;
    println!("Lean {} ({})", analysis.toolchain.lean_version, analysis.toolchain.lean_githash);
    println!(
        "{} modules, {} compiled declarations, {} externs, {} exports",
        s.module_graph.len(),
        s.bir.declarations.len(),
        s.extern_requirements.len(),
        s.interface.exports.len()
    );
    for w in &analysis.warnings {
        println!("warning: {}", w.message);
    }
}

/// Prints what is known about `name`; source paths are shown relative to the configuration
/// (`local_prefix` is the Lean project's path from it), as in generated code.
fn inspect(analysis: &Analysis, name: &str, local_prefix: &str) -> Result<()> {
    let s = &analysis.success;
    let mut found = false;
    if let Some(e) = s.interface.exports.iter().find(|e| e.name == name) {
        found = true;
        println!("{name} : {}", e.lean_type);
        println!("module: {}", e.module);
        if let Some(src) = &e.source {
            println!("source: {}", lungo_build::codegen::display_path(&src.location, local_prefix));
        }
        println!("exported: yes");
        println!("axioms: {}", e.trust.axioms.join(", "));
        println!("depends on sorry: {}", e.trust.depends_on_sorry);
        println!("unsafe dependencies: {}", e.trust.unsafe_dependencies.join(", "));
        println!("partial dependencies: {}", e.trust.partial_dependencies.join(", "));
        println!("extern dependencies: {}", e.trust.extern_dependencies.join(", "));
    }
    if let Some(d) = s.bir.declaration(name) {
        found = true;
        let params: Vec<String> =
            d.params.iter().map(|p| format!("{}{}", if p.borrow { "@& " } else { "" }, p.ty.name())).collect();
        println!("compiled: ({}) -> {} in module {}", params.join(", "), d.result.name(), d.module);
        let aux: Vec<&str> = s
            .bir
            .declarations
            .iter()
            .filter(|x| x.name != name && x.origin.as_deref() == Some(name))
            .map(|x| x.name.as_str())
            .collect();
        if !aux.is_empty() {
            println!("compiler auxiliaries: {}", aux.join(", "));
        }
    }
    if let Some(r) = s.extern_requirements.iter().find(|r| r.declaration == name) {
        found = true;
        println!("extern: {:?}", r.entry);
    }
    if !found {
        return Err(Error::Configuration(format!("{name} is neither exported nor part of the compiled program")));
    }
    Ok(())
}

fn ir(analysis: &Analysis, name: &str) -> Result<()> {
    let s = &analysis.success;
    let decls: Vec<&lungo_build::bir::Declaration> =
        s.bir.declarations.iter().filter(|d| d.name == name || d.origin.as_deref() == Some(name)).collect();
    if decls.is_empty() {
        return Err(Error::Configuration(format!("{name} has no compiled code in the program")));
    }
    for d in decls {
        print!("{}", lungo_build::bir::pretty_declaration(d));
    }
    Ok(())
}

fn rust(analysis: &Analysis, generation: &lungo_build::Generation, name: &str, aggregate: &str) -> Result<()> {
    let d = analysis
        .success
        .bir
        .declaration(name)
        .ok_or_else(|| Error::Configuration(format!("{name} has no compiled code in the program")))?;
    let file = format!("modules/{}.rs", lungo_build::codegen::module_file_stem(&d.module));
    let module =
        generation.files.get(&file).ok_or_else(|| Error::Configuration(format!("{file} was not generated")))?;
    if !print_items(module, &format!("// Lean: {name}")) {
        return Err(Error::Configuration(format!("no generated Rust found for {name} in {file}")));
    }
    // The facade function, found by its Rust path: its documentation may be disabled.
    let names = generation.files.get("names.json").ok_or(Error::Configuration("no name mappings generated".into()))?;
    let records: Vec<serde_json::Value> =
        serde_json::from_str(names).map_err(|e| Error::Configuration(format!("names.json: {e}")))?;
    let facade = records.iter().find(|r| r["lean_name"] == name && r["kind"] == "function");
    if let Some(path) = facade.and_then(|r| r["rust_path"].as_str()) {
        let text = generation
            .files
            .get(&format!("{aggregate}.rs"))
            .ok_or(Error::Configuration("the aggregate module was not generated".into()))?;
        let item = facade_function(text, path)
            .ok_or_else(|| Error::Configuration(format!("the facade function {path} was not found")))?;
        println!("{item}");
    }
    Ok(())
}

/// Prints each item introduced by a line starting with `marker`, up to the next blank line.
fn print_items(text: &str, marker: &str) -> bool {
    let mut printed = false;
    let mut printing = false;
    for line in text.lines() {
        if line.trim_start().starts_with(marker) {
            printing = true;
            printed = true;
        }
        if printing {
            if line.trim().is_empty() {
                printing = false;
                println!();
            } else {
                println!("{line}");
            }
        }
    }
    printed
}

/// The item (with its documentation and attributes) defining the facade function at `path`, such
/// as `eval_tokens::go`, in a generated aggregate module. Items end at a blank line, and
/// `pub mod m {` opens a module whose closing brace is at the same indentation.
fn facade_function(text: &str, path: &str) -> Option<String> {
    let (modules, function) = match path.rsplit_once("::") {
        Some((modules, function)) => (modules.split("::").collect(), function),
        None => (Vec::new(), path),
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut open: Vec<(&str, usize)> = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let indent = line.len() - line.trim_start().len();
        let body = line.trim_start();
        if let Some(name) = body.strip_prefix("pub mod ").and_then(|rest| rest.strip_suffix(" {")) {
            open.push((name, indent));
        } else if body == "}" && open.last().is_some_and(|&(_, at)| at == indent) {
            open.pop();
        } else if indent == 4 * open.len()
            && open.iter().map(|&(name, _)| name).eq(modules.iter().copied())
            && body
                .strip_prefix("pub fn ")
                .and_then(|rest| rest.strip_prefix(function))
                .is_some_and(|rest| rest.starts_with('('))
        {
            let preamble = |l: &&&str| {
                let body = l.trim_start();
                l.len() - body.len() == indent && (body.starts_with("///") || body.starts_with("#["))
            };
            let start = i - lines[..i].iter().rev().take_while(preamble).count();
            let end = lines[i..].iter().position(|l| l.trim().is_empty()).map_or(lines.len(), |e| i + e);
            return Some(lines[start..end].join("\n"));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn language_flags_are_split_in_both_spellings() {
        let (rest, flags) = split_language_flags(args(&[
            "lungo",
            "generate",
            "--go_out=gen/go",
            "--python_out",
            "gen/py",
            "--go_opt=package=formal,x=1",
            "--runtime-dir",
            "dist",
        ]))
        .unwrap();
        assert_eq!(rest, args(&["lungo", "generate", "--runtime-dir", "dist"]));
        assert_eq!(flags.outs, vec![("go".into(), "gen/go".into()), ("python".into(), "gen/py".into())]);
        assert_eq!(
            flags.opts,
            vec![("go".into(), "package".into(), "formal".into()), ("go".into(), "x".into(), "1".into())]
        );
        assert!(split_language_flags(args(&["lungo", "--go_opt=nokey"])).is_err());
        assert!(split_language_flags(args(&["lungo", "--Go_out=x"])).is_err());
        assert!(split_language_flags(args(&["lungo", "--go_out"])).is_err());
    }

    #[test]
    fn facade_functions_are_found_by_rust_path() {
        const AGGREGATE: &str = "\
/// Lean: `Host.evalTokens`
#[allow(unused_imports)]
pub fn eval_tokens(ts: List) -> Nat {
    go(ts)
}

pub mod eval_tokens {
    #[allow(unused_imports)]
    pub fn go(ts: List) -> Nat {
        inner()
    }

    pub fn go_on(ts: List) -> Nat {
        inner()
    }

}

pub fn go(n: Nat) -> Nat {
    n
}
";
        let top = facade_function(AGGREGATE, "eval_tokens").unwrap();
        assert!(top.starts_with("/// Lean: `Host.evalTokens`") && top.ends_with("    go(ts)\n}"), "{top}");
        let nested = facade_function(AGGREGATE, "eval_tokens::go").unwrap();
        assert!(nested.starts_with("    #[allow(unused_imports)]\n    pub fn go(ts: List)"), "{nested}");
        assert!(facade_function(AGGREGATE, "go").unwrap().starts_with("pub fn go(n: Nat)"));
        assert!(facade_function(AGGREGATE, "eval_tokens::go_on").unwrap().contains("pub fn go_on("));
        assert_eq!(facade_function(AGGREGATE, "eval_tokens::missing"), None);
        assert_eq!(facade_function(AGGREGATE, "other::go"), None);
    }
}
