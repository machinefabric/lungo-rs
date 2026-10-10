//! Driving Lake: the project's own build system elaborates, checks, and compiles the Lean
//! modules. lungo never parses Lean source or rewrites project files.

use crate::error::{Error, Result};
use crate::fingerprint::hash_bytes;
use crate::toolchain::Toolchain;
use lungo_protocol::Roots;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Files that describe the project and must not change during a build.
pub fn configuration_files(project: &Path) -> Vec<PathBuf> {
    ["lean-toolchain", "lakefile.lean", "lakefile.toml", "lake-manifest.json"]
        .iter()
        .map(|f| project.join(f))
        .filter(|p| p.is_file())
        .collect()
}

/// A snapshot of the configuration files' contents.
pub fn snapshot(project: &Path) -> Result<Vec<(PathBuf, String)>> {
    configuration_files(project)
        .into_iter()
        .map(|p| {
            let bytes = std::fs::read(&p).map_err(|e| Error::io(format!("cannot read {}", p.display()), e))?;
            Ok((p, hash_bytes(&bytes)))
        })
        .collect()
}

#[derive(Deserialize)]
struct Manifest {
    /// The root package's name.
    name: String,
    #[serde(rename = "packagesDir", default)]
    packages_dir: Option<String>,
    #[serde(default)]
    packages: Vec<ManifestPackage>,
}

#[derive(Deserialize)]
struct ManifestPackage {
    name: String,
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    dir: Option<String>,
}

/// What the build needs to know about a Lake project before Lake runs.
pub struct LakeProject {
    /// The root package's name, as the committed manifest records it.
    pub name: String,
}

/// Checks that the project is a complete Lake project whose locked dependencies are all
/// materialized, so that building it requires no network access and no manifest changes.
pub fn validate_project(project: &Path) -> Result<LakeProject> {
    if !project.join("lakefile.toml").is_file() && !project.join("lakefile.lean").is_file() {
        return Err(Error::Project(format!("{} has neither lakefile.toml nor lakefile.lean", project.display())));
    }
    let manifest_path = project.join("lake-manifest.json");
    let bytes = std::fs::read(&manifest_path).map_err(|_| {
        Error::Project(format!(
            "{} is missing; run `lake update` once and commit the manifest",
            manifest_path.display()
        ))
    })?;
    let manifest: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| Error::Project(format!("{} is not a valid Lake manifest: {e}", manifest_path.display())))?;
    let packages_dir = project.join(manifest.packages_dir.as_deref().unwrap_or(".lake/packages"));
    for p in &manifest.packages {
        let dir = match p.kind.as_str() {
            "git" => packages_dir.join(&p.name),
            "path" => project.join(p.dir.as_deref().ok_or_else(|| {
                Error::Project(format!("path dependency {} has no directory in the manifest", p.name))
            })?),
            other => return Err(Error::Project(format!("unknown Lake package type {other:?} for {}", p.name))),
        };
        if !dir.is_dir() {
            return Err(Error::Project(format!(
                "dependency {} is not materialized at {}; fetch it explicitly with `lake update` or `lungo setup` — builds never fetch dependencies",
                p.name,
                dir.display()
            )));
        }
    }
    Ok(LakeProject { name: manifest.name })
}

/// Builds the root modules (and everything they import) with Lake: the given modules, or the
/// package's default targets.
pub fn build(
    toolchain: &Toolchain,
    project: &Path,
    roots: &Roots,
    assurance_modules: &[String],
    offline: bool,
) -> Result<()> {
    // Naming modules replaces the default targets, so the default targets are one build and the
    // assurance modules another.
    match roots {
        Roots::Modules(modules) => {
            let all: Vec<String> = modules.iter().chain(assurance_modules).cloned().collect();
            build_targets(toolchain, project, &all, offline)
        }
        Roots::DefaultTargets => {
            build_targets(toolchain, project, &[], offline)?;
            if assurance_modules.is_empty() { Ok(()) } else { build_targets(toolchain, project, assurance_modules, offline) }
        }
    }
}

/// `lake build` of `modules`, or of the default targets when there are none.
fn build_targets(toolchain: &Toolchain, project: &Path, modules: &[String], offline: bool) -> Result<()> {
    let before = snapshot(project)?;
    let mut cmd = Command::new(&toolchain.lake);
    cmd.arg("build");
    if offline {
        cmd.arg("--no-cache");
    }
    for r in modules {
        cmd.arg(format!("+{r}"));
    }
    cmd.current_dir(project);
    for var in ["LEAN_PATH", "LEAN_SRC_PATH", "LEAN_SYSROOT", "LAKE", "LAKE_HOME", "ELAN_TOOLCHAIN"] {
        cmd.env_remove(var);
    }
    let out = cmd.output().map_err(|e| Error::io("cannot run lake", e))?;
    let output = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let after = snapshot(project)?;
    if before != after {
        return Err(Error::Project(format!(
            "Lake modified the project configuration during the build ({}); lungo builds must not change project files",
            after
                .iter()
                .filter(|a| !before.contains(a))
                .map(|(p, _)| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )));
    }
    if !out.status.success() {
        return Err(Error::LeanElaboration { output });
    }
    Ok(())
}
