//! `LeanOracle` mode: the facade runs Lean's official native backend and runtime.
//!
//! Lake produces the C code of every non-toolchain module with Lean's own C emitter; the Lean
//! toolchain's `leanc` compiles it together with a small shim exposing `lean.h`'s inline
//! allocation and reference-counting functions; the result links against the toolchain's static
//! Lean runtime and libraries. The generated facade is identical to PureRust mode's apart from
//! the object backend. This mode exists to serve as a reference oracle.

use crate::{Analysis, Builder, Context, Environment, Error, Generation, Result};
use lungo_protocol::PackageOrigin;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &mut Command, what: &str) -> Result<String> {
    let out = cmd.output().map_err(|e| Error::io(format!("cannot run {what}"), e))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if !out.status.success() {
        return Err(Error::Command { program: what.to_owned(), status: out.status.to_string(), output: text });
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn tool(ctx: &Context, name: &str) -> PathBuf {
    ctx.toolchain.root.join("bin").join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

pub(crate) fn generate(
    cfg: &Builder,
    ctx: &Context,
    env: &Environment,
    analysis: &Analysis,
    namespace: &str,
    shaping: &lungo_codegen::Shaping,
    embedded: Option<&BTreeMap<String, String>>,
) -> Result<Generation> {
    if env.target.triple != env.host {
        return Err(Error::Environment(format!(
            "LeanOracle mode runs Lean's native code and cannot cross-compile (host {}, target {})",
            env.host, env.target.triple
        )));
    }
    if env.target.triple.contains("msvc") {
        return Err(Error::Environment(
            "LeanOracle mode links Lean's native runtime, which is built for the GNU ABI on Windows; use a `*-windows-gnu` target".into(),
        ));
    }
    // C code of every linked module outside the toolchain, produced by Lean's C emitter through
    // Lake. Modules only `meta`-imported are compile-time code and are not linked.
    let linked: BTreeSet<&str> = analysis.success.bir.modules.iter().map(|m| m.name.as_str()).collect();
    let modules: Vec<&str> = analysis
        .success
        .module_graph
        .iter()
        .filter(|m| linked.contains(m.name.as_str()))
        .filter(|m| m.source.as_ref().is_some_and(|s| s.origin != PackageOrigin::Toolchain))
        .map(|m| m.name.as_str())
        .collect();
    let targets: Vec<String> = modules.iter().map(|m| format!("+{m}:c")).collect();
    let lake = &ctx.toolchain.lake;
    run(Command::new(lake).arg("build").args(&targets).current_dir(&ctx.project), "lake build (C code)")?;
    let listing = run(
        Command::new(lake).arg("query").arg("--text").args(&targets).current_dir(&ctx.project),
        "lake query (C code)",
    )?;
    let c_files: Vec<PathBuf> =
        listing.lines().filter(|l| !l.trim().is_empty()).map(|l| PathBuf::from(l.trim())).collect();
    if c_files.len() != modules.len() {
        return Err(Error::Environment(format!(
            "lake reported {} C files for {} modules",
            c_files.len(),
            modules.len()
        )));
    }
    let objects_dir = ctx.work_dir.join("oracle-objects");
    if objects_dir.exists() {
        std::fs::remove_dir_all(&objects_dir).map_err(|e| Error::io("cannot clear oracle objects", e))?;
    }
    std::fs::create_dir_all(&objects_dir).map_err(|e| Error::io("cannot create oracle objects", e))?;
    let shim = objects_dir.join("lungo_oracle_shim.c");
    std::fs::write(&shim, lungo_codegen::rust::ORACLE_C_SHIM)
        .map_err(|e| Error::io("cannot write the oracle shim", e))?;
    let leanc = tool(ctx, "leanc");
    let mut objects = Vec::new();
    for (i, c) in c_files.iter().chain(std::iter::once(&shim)).enumerate() {
        let obj = objects_dir.join(format!("m{i}.o"));
        run(
            Command::new(&leanc).args(["-c", "-O3", "-DNDEBUG"]).arg(c).arg("-o").arg(&obj),
            &format!("leanc -c {}", c.display()),
        )?;
        objects.push(obj);
    }
    // One archive per generated module, so that a crate may link several.
    let library = format!("lungo_oracle_{}", ctx.name.replace('-', "_"));
    let archive = objects_dir.join(format!("lib{library}.a"));
    run(Command::new(tool(ctx, "llvm-ar")).arg("rcs").arg(&archive).args(&objects), "llvm-ar")?;
    let archive_bytes = std::fs::read(&archive).map_err(|e| Error::io("cannot read the oracle archive", e))?;
    let link_directives = link_directives(&leanc, &ctx.toolchain.root, &ctx.out_dir.join("native"), &library)?;
    let input = lungo_codegen::rust::GenInput {
        layer: lungo_codegen::rust::Layer::Oracle,
        success: &analysis.success,
        toolchain: &analysis.toolchain,
        facade_namespace: namespace,
        aggregate: &ctx.name,
        rust_externs: &cfg.rust.rust_externs,
        local_prefix: &ctx.local_prefix,
        embedded_sources: embedded,
        shaping,
    };
    let generated = lungo_codegen::rust::generate(&input).map_err(|errors| Error::Codegen {
        toolchain: format!("v{}", analysis.toolchain.lean_version),
        bir_version: analysis.success.bir.bir_version,
        errors,
    })?;
    let mut binary_files = BTreeMap::new();
    binary_files.insert(format!("native/lib{library}.a"), archive_bytes);
    Ok(Generation { files: generated.files, binary_files, link_directives })
}

/// Cargo directives linking the oracle archive and the Lean toolchain's native libraries.
///
/// The toolchain's own link flags (`leanc --print-ldflags`) are passed verbatim and in order,
/// preserving library order and grouping. Cargo applies such linker arguments to the targets of
/// the package whose build script generated the code, which is where oracle code is used.
fn link_directives(leanc: &Path, root: &Path, native_dir: &Path, library: &str) -> Result<Vec<String>> {
    let flags = run(Command::new(leanc).arg("--print-ldflags"), "leanc --print-ldflags")?;
    let mut out = vec![
        format!("cargo::rustc-link-search=native={}", native_dir.display()),
        format!("cargo::rustc-link-lib=static:-bundle={library}"),
        format!("cargo::rustc-link-arg=-L{}", root.join("lib").display()),
    ];
    let mut tokens = flags.split_whitespace().peekable();
    while let Some(t) = tokens.next() {
        if t == "-L" {
            let dir = tokens.next().ok_or_else(|| Error::Environment("`leanc --print-ldflags` ends with -L".into()))?;
            out.push(format!("cargo::rustc-link-arg=-L{dir}"));
        } else if t.starts_with("-L") || t.starts_with("-l") || t.starts_with("-Wl,") {
            out.push(format!("cargo::rustc-link-arg={t}"));
        }
    }
    Ok(out)
}
