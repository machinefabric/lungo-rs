//! `cargo lean2rust`: inspect and build Lean projects integrated with lean2rust.
//!
//! Every command runs the same library pipeline as `build.rs` (`lean2rust_build::Config`), so
//! the CLI never produces output that differs from Cargo generation. The configuration is read
//! from `lean2rust.toml` in the package directory, or given on the command line.

use clap::{Args, Parser, Subcommand};
use lean2rust_build::{Analysis, Config, Environment, Mode};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "cargo", bin_name = "cargo")]
enum Cargo {
    /// Inspect and build Lean projects integrated with lean2rust.
    Lean2rust(Cli),
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
    /// A lean2rust configuration file (default: `<package>/lean2rust.toml` when it exists).
    #[arg(long, global = true)]
    config: Option<PathBuf>,
    /// The Lake project directory, relative to the package (instead of a configuration file).
    #[arg(long, global = true)]
    project: Option<PathBuf>,
    /// A root module (repeatable).
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
    /// Where generated files are written (default: `<package>/target/lean2rust/out`).
    #[arg(long, global = true)]
    out_dir: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    /// Build the Lean project and check that it translates, without writing output.
    Check,
    /// Generate the Rust output into the output directory.
    Build,
    /// Show what lean2rust knows about a declaration.
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
    let Cargo::Lean2rust(cli) = Cargo::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

fn load_config(opts: &Options, package: &Path) -> Result<Config> {
    let file = opts.config.clone().or_else(|| {
        let default = package.join("lean2rust.toml");
        default.is_file().then_some(default)
    });
    let mut cfg = match (&file, &opts.project) {
        (Some(path), None) => Config::load(path)?,
        (None, Some(project)) => Config::new(project),
        (Some(_), Some(_)) => return Err("give either a configuration file or --project, not both".into()),
        (None, None) => {
            return Err(format!(
                "no lean2rust configuration: create {} or pass --project and --root",
                package.join("lean2rust.toml").display()
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
    Ok(cfg)
}

fn environment(opts: &Options, package: &Path) -> Environment {
    let base = package.join("target").join("lean2rust");
    let out = opts.out_dir.clone().unwrap_or_else(|| base.join("out"));
    Environment::native(package.to_path_buf(), out, base.join("work"))
}

fn run(cli: Cli) -> Result<()> {
    let package = match &cli.options.package_dir {
        Some(p) => std::fs::canonicalize(p)?,
        None => std::env::current_dir()?,
    };
    let env = environment(&cli.options, &package);
    if let Command::Setup = cli.command {
        return setup(&cli.options, &package, &env);
    }
    let cfg = load_config(&cli.options, &package)?;
    match cli.command {
        Command::Check => {
            let analysis = cfg.analyze(&env)?;
            let generation = cfg.generate(&env, &analysis)?;
            report_summary(&analysis);
            println!("generation succeeded: {} files", generation.files.len() + generation.binary_files.len());
        }
        Command::Build => {
            let outcome = cfg.run(&env)?;
            println!("{} {}", if outcome.reused { "up to date:" } else { "generated" }, env.out_dir.display());
        }
        Command::Inspect { declaration } => {
            let ctx = cfg.context(&env)?;
            inspect(&cfg.analyze(&env)?, &declaration, &ctx.local_prefix)?
        }
        Command::Ir { declaration } => ir(&cfg.analyze(&env)?, &declaration)?,
        Command::Rust { declaration } => {
            let analysis = cfg.analyze(&env)?;
            let generation = cfg.generate(&env, &analysis)?;
            rust(&analysis, &generation, &declaration, &cfg.aggregate_name())?;
        }
        Command::Externs => {
            let analysis = cfg.analyze(&env)?;
            let generation = cfg.generate(&env, &analysis)?;
            print!("{}", generation.files.get("externs.json").ok_or("no extern report generated")?);
        }
        Command::Mappings => {
            let analysis = cfg.analyze(&env)?;
            let generation = cfg.generate(&env, &analysis)?;
            print!("{}", generation.files.get("names.json").ok_or("no name mappings generated")?);
        }
        Command::Prepare => {
            let ctx = cfg.context(&env)?;
            let worker = cfg.prepare_worker(&ctx, &env)?;
            println!("Lean {} ({})", ctx.toolchain.version, ctx.toolchain.githash);
            println!("worker {} at {}", worker.identity, worker.binary.display());
        }
        Command::Setup => unreachable!("handled above"),
    }
    Ok(())
}

fn setup(opts: &Options, package: &Path, env: &Environment) -> Result<()> {
    let cfg = load_config(opts, package)?.toolchain_policy(lean2rust_build::ToolchainPolicy::Install);
    let project = if cfg.project.is_absolute() { cfg.project.clone() } else { package.join(&cfg.project) };
    let pin = lean2rust_build::read_pin(&project)?;
    let toolchain = lean2rust_build::Toolchain::resolve_pin(
        &pin,
        cfg.toolchain_dir.as_deref(),
        lean2rust_build::ToolchainPolicy::Install,
    )?;
    // Loading the workspace materializes the dependencies locked in the manifest.
    let status = std::process::Command::new(&toolchain.lake)
        .args(["env", "lean", "--version"])
        .current_dir(&project)
        .status()?;
    if !status.success() {
        return Err(format!("materializing Lake dependencies failed ({status})").into());
    }
    let ctx = cfg.context(env)?;
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
            println!("source: {}", lean2rust_codegen::display_path(&src.location, local_prefix));
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
    let decls: Vec<&lean2rust_bir::Declaration> =
        s.bir.declarations.iter().filter(|d| d.name == name || d.origin.as_deref() == Some(name)).collect();
    if decls.is_empty() {
        return Err(format!("{name} has no compiled code in the program").into());
    }
    for d in decls {
        print!("{}", lean2rust_bir::pretty_declaration(d));
    }
    Ok(())
}

fn rust(analysis: &Analysis, generation: &lean2rust_build::Generation, name: &str, aggregate: &str) -> Result<()> {
    let d =
        analysis.success.bir.declaration(name).ok_or_else(|| format!("{name} has no compiled code in the program"))?;
    let mut printed = false;
    let file = format!("modules/{}.rs", lean2rust_codegen::module_file_stem(&d.module));
    if let Some(text) = generation.files.get(&file) {
        printed |= print_items(text, &format!("// Lean: {name}"));
    }
    if let Some(text) = generation.files.get(&format!("{aggregate}.rs")) {
        printed |= print_items(text, &format!("/// Lean: `{name} :"));
    }
    if !printed {
        return Err(format!("no generated Rust found for {name}").into());
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
