//! `cargo patina`: inspect and build Lean projects integrated with patina.
//!
//! Every command runs the same library pipeline as `build.rs` (`patina_build::Builder`), so
//! the CLI never produces output that differs from Cargo generation. The configuration is read
//! from `patina.toml` in the package directory (see `patina_build::ProjectFile`), or given on
//! the command line.

use clap::{Args, Parser, Subcommand};
use patina_build::{Analysis, Builder, Environment, Mode, ProjectFile};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    /// Inspect and build Lean projects integrated with patina.
    Patina(Cli),
}

#[derive(Args)]
#[command(version)]
struct Cli {
    #[command(flatten)]
    options: Options,
    #[command(subcommand)]
    command: Command,
}

#[derive(Args, Clone)]
struct Options {
    /// The Cargo package directory (default: the current directory).
    #[arg(long, global = true)]
    package_dir: Option<PathBuf>,
    /// A patina configuration file (default: `<package>/patina.toml` when it exists).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// The Lake project directory, relative to the package (instead of a configuration file).
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
    /// Use Lean's native backend (`LeanOracle`) instead of PureRust.
    #[arg(long, global = true)]
    oracle: bool,
    /// Where generated modules are written, each as `<out-dir>/<name>` (default: the
    /// configuration's `out-dir`, else `<package>/target/patina/out`).
    #[arg(long, global = true)]
    out_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Build the Lean project and check that it translates, without writing output.
    Check,
    /// Generate the Rust output into the output directory.
    Build,
    /// Show what patina knows about a declaration.
    Inspect { declaration: String },
    /// Print the Bridge IR of a declaration and the compiler auxiliaries derived from it.
    Ir { declaration: String },
    /// Print the generated Rust of a declaration.
    Rust { declaration: String },
    /// List every extern the program reaches and how it is implemented.
    Externs,
    /// List the Lean-name to Rust-path mappings of the public facade.
    Mappings,
    /// Build (or verify) the Lean worker for the project's toolchain.
    Prepare,
    /// Explicitly install the pinned toolchain and materialize locked Lake dependencies.
    Setup,
}

fn main() -> ExitCode {
    let Cargo::Patina(cli) = Cargo::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

/// The Lake project and build settings: `patina.toml` (or `--config`), or `--project`, with the
/// command-line options added.
fn load_config(opts: &Options, package: &Path) -> Result<(PathBuf, Builder)> {
    let file = opts.config.clone().or_else(|| {
        let default = package.join("patina.toml");
        default.is_file().then_some(default)
    });
    let (project, mut cfg) = match (&file, &opts.project) {
        (Some(path), None) => {
            let file = ProjectFile::load(path)?;
            (file.project, file.build)
        }
        (None, Some(project)) => (project.clone(), patina_build::configure()),
        (Some(_), Some(_)) => return Err("give either a configuration file or --project, not both".into()),
        (None, None) => {
            return Err(format!(
                "no patina configuration: create {} or pass --project",
                package.join("patina.toml").display()
            )
            .into());
        }
    };
    for r in &opts.roots {
        cfg = cfg.root_module(r);
    }
    for e in &opts.exports {
        cfg = cfg.export(e);
    }
    for m in &opts.export_modules {
        cfg = cfg.export_module(m);
    }
    if opts.oracle {
        cfg = cfg.mode(Mode::LeanOracle);
    }
    Ok((project, cfg))
}

/// Where the command works: `--out-dir`, else the configuration's `out-dir` (relative to the
/// package), else `<package>/target/patina/out`.
fn environment(opts: &Options, cfg: &Builder, package: &Path) -> Environment {
    let base = package.join("target").join("patina");
    let out = match (&opts.out_dir, &cfg.out_dir) {
        (Some(dir), _) => dir.clone(),
        (None, Some(dir)) => package.join(dir),
        (None, None) => base.join("out"),
    };
    Environment::native(package.to_path_buf(), out, base.join("work"))
}

fn run(cli: Cli) -> Result<()> {
    let package = match &cli.options.package_dir {
        Some(p) => patina_build::canonical_path(p)?,
        None => std::env::current_dir()?,
    };
    let (project, cfg) = load_config(&cli.options, &package)?;
    let env = environment(&cli.options, &cfg, &package);
    if let Command::Setup = cli.command {
        return setup(project, cfg, &package, &env);
    }
    let project = project.as_path();
    match cli.command {
        Command::Check => {
            let analysis = cfg.analyze(project, &env)?;
            let generation = cfg.generate(project, &env, &analysis)?;
            report_summary(&analysis);
            println!("generation succeeded: {} files", generation.files.len() + generation.binary_files.len());
        }
        Command::Build => {
            let outcome = cfg.run(project, &env)?;
            let dir = env.out_dir.join(&outcome.name);
            println!("{} {}", if outcome.reused { "up to date:" } else { "generated" }, dir.display());
        }
        Command::Inspect { declaration } => {
            let ctx = cfg.context(project, &env)?;
            inspect(&cfg.analyze(project, &env)?, &declaration, &ctx.local_prefix)?
        }
        Command::Ir { declaration } => ir(&cfg.analyze(project, &env)?, &declaration)?,
        Command::Rust { declaration } => {
            let ctx = cfg.context(project, &env)?;
            let analysis = cfg.analyze(project, &env)?;
            let generation = cfg.generate(project, &env, &analysis)?;
            rust(&analysis, &generation, &declaration, &ctx.name)?;
        }
        Command::Externs => {
            let analysis = cfg.analyze(project, &env)?;
            let generation = cfg.generate(project, &env, &analysis)?;
            print!("{}", generation.files.get("externs.json").ok_or("no extern report generated")?);
        }
        Command::Mappings => {
            let analysis = cfg.analyze(project, &env)?;
            let generation = cfg.generate(project, &env, &analysis)?;
            print!("{}", generation.files.get("names.json").ok_or("no name mappings generated")?);
        }
        Command::Prepare => {
            let ctx = cfg.context(project, &env)?;
            let worker = cfg.prepare_worker(&ctx, &env)?;
            println!("Lean {} ({})", ctx.toolchain.version, ctx.toolchain.githash);
            println!("worker {} at {}", worker.identity, worker.binary.display());
        }
        Command::Setup => unreachable!("handled above"),
    }
    Ok(())
}

fn setup(project: PathBuf, cfg: Builder, package: &Path, env: &Environment) -> Result<()> {
    let cfg = cfg.toolchain_policy(patina_build::ToolchainPolicy::Install);
    let project = if project.is_absolute() { project } else { package.join(project) };
    let pin = patina_build::read_pin(&project)?;
    let toolchain = patina_build::Toolchain::resolve_pin(
        &pin,
        cfg.toolchain_dir.as_deref(),
        patina_build::ToolchainPolicy::Install,
    )?;
    // Loading the workspace materializes the dependencies locked in the manifest.
    let status = std::process::Command::new(&toolchain.lake)
        .args(["env", "lean", "--version"])
        .current_dir(&project)
        .status()?;
    if !status.success() {
        return Err(format!("materializing Lake dependencies failed ({status})").into());
    }
    let ctx = cfg.context(&project, env)?;
    let worker = cfg.prepare_worker(&ctx, env)?;
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

/// Prints what is known about `name`; source paths are shown relative to the Cargo package
/// (`local_prefix` is the Lean project's path within it), as in generated code.
fn inspect(analysis: &Analysis, name: &str, local_prefix: &str) -> Result<()> {
    let s = &analysis.success;
    let mut found = false;
    if let Some(e) = s.interface.exports.iter().find(|e| e.name == name) {
        found = true;
        println!("{name} : {}", e.lean_type);
        println!("module: {}", e.module);
        if let Some(src) = &e.source {
            println!("source: {}", patina_codegen::display_path(&src.location, local_prefix));
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
        return Err(format!("{name} is neither exported nor part of the compiled program").into());
    }
    Ok(())
}

fn ir(analysis: &Analysis, name: &str) -> Result<()> {
    let s = &analysis.success;
    let decls: Vec<&patina_bir::Declaration> =
        s.bir.declarations.iter().filter(|d| d.name == name || d.origin.as_deref() == Some(name)).collect();
    if decls.is_empty() {
        return Err(format!("{name} has no compiled code in the program").into());
    }
    for d in decls {
        print!("{}", patina_bir::pretty_declaration(d));
    }
    Ok(())
}

fn rust(analysis: &Analysis, generation: &patina_build::Generation, name: &str, aggregate: &str) -> Result<()> {
    let d =
        analysis.success.bir.declaration(name).ok_or_else(|| format!("{name} has no compiled code in the program"))?;
    let file = format!("modules/{}.rs", patina_codegen::module_file_stem(&d.module));
    let module = generation.files.get(&file).ok_or_else(|| format!("{file} was not generated"))?;
    if !print_items(module, &format!("// Lean: {name}")) {
        return Err(format!("no generated Rust found for {name} in {file}").into());
    }
    // The facade function, found by its Rust path: its documentation may be disabled.
    let names = generation.files.get("names.json").ok_or("no name mappings generated")?;
    let records: Vec<serde_json::Value> = serde_json::from_str(names)?;
    let facade = records.iter().find(|r| r["lean_name"] == name && r["kind"] == "function");
    if let Some(path) = facade.and_then(|r| r["rust_path"].as_str()) {
        let text = generation.files.get(&format!("{aggregate}.rs")).ok_or("the aggregate module was not generated")?;
        let item = facade_function(text, path).ok_or_else(|| format!("the facade function {path} was not found"))?;
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
    use super::facade_function;

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

    #[test]
    fn facade_functions_are_found_by_rust_path() {
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
