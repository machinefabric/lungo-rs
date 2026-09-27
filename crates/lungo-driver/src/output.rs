//! Publication of generated artifacts and the build record.

use crate::error::{Error, Result};
use crate::fingerprint::hash_bytes;
use crate::{Analysis, BuildKey, Context, relative_path};
use lungo_protocol::{PackageOrigin, Success};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// The record of one generation (`build-info.json`): the build key and everything it covers.
/// Paths are relative, so the record carries no machine-specific absolute paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildInfo {
    pub build_key: String,
    pub lean_version: String,
    pub lean_githash: String,
    pub bir_version: u32,
    pub adapter_version: u32,
    pub lungo_version: String,
    pub runtime_abi: u32,
    pub worker_identity: String,
    /// Input files, relative to the Lean project directory, with their content digests.
    pub input_digests: BTreeMap<String, String>,
    /// The Lean project directory, relative to the Cargo package.
    pub project: String,
    pub link_directives: Vec<String>,
    /// The generated files, relative to the output directory, with their content digests: an
    /// output is reused only while it still holds exactly these.
    pub output_digests: BTreeMap<String, String>,
}

impl BuildInfo {
    pub fn new(
        key: &BuildKey,
        ctx: &Context,
        analysis: &Analysis,
        inputs: &[PathBuf],
        link_directives: &[String],
    ) -> Result<Self> {
        let mut input_digests = BTreeMap::new();
        for p in inputs {
            let bytes = fs::read(p).map_err(|e| Error::io(format!("cannot read input {}", p.display()), e))?;
            input_digests.insert(relative_path(&ctx.project, p), hash_bytes(&bytes));
        }
        Ok(BuildInfo {
            build_key: key.value.clone(),
            lean_version: analysis.toolchain.lean_version.clone(),
            lean_githash: analysis.toolchain.lean_githash.clone(),
            bir_version: analysis.success.bir.bir_version,
            adapter_version: analysis.toolchain.adapter_version,
            lungo_version: env!("CARGO_PKG_VERSION").into(),
            runtime_abi: lungo_runtime::ABI_VERSION,
            worker_identity: ctx.worker_identity.clone(),
            input_digests,
            project: ctx.local_prefix.clone(),
            link_directives: link_directives.to_vec(),
            output_digests: BTreeMap::new(),
        })
    }
}

/// A previous build record, with input paths resolved against the project directory.
pub struct PreviousBuild {
    pub build_key: String,
    pub inputs: Vec<String>,
    pub digests: Vec<(PathBuf, String)>,
    pub link_directives: Vec<String>,
    pub output_digests: BTreeMap<String, String>,
}

/// The record of how an output directory was generated.
pub const BUILD_INFO: &str = "build-info.json";

/// Reads the build record of the output in `out_dir`, if it exists and is intact, resolving its
/// input paths against the Lean project directory `project`.
pub fn read_build_info(out_dir: &Path, project: &Path) -> Option<PreviousBuild> {
    let info: BuildInfo = serde_json::from_slice(&fs::read(out_dir.join(BUILD_INFO)).ok()?).ok()?;
    let digests: Vec<(PathBuf, String)> =
        info.input_digests.iter().map(|(rel, d)| (project.join(rel), d.clone())).collect();
    Some(PreviousBuild {
        build_key: info.build_key,
        inputs: digests.iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect(),
        digests,
        link_directives: info.link_directives,
        output_digests: info.output_digests,
    })
}

/// Whether the previous build in `out_dir` is still current: every input has the recorded
/// contents, and the directory holds exactly the files that build generated. A generated file
/// edited, removed or added by hand makes it stale, so it is generated again.
pub fn still_current(out_dir: &Path, previous: &PreviousBuild) -> bool {
    let inputs = previous.digests.iter().all(|(p, d)| fs::read(p).map(|b| hash_bytes(&b) == *d).unwrap_or(false));
    inputs
        && match output_files(out_dir) {
            Ok(files) => {
                files.len() == previous.output_digests.len()
                    && files.iter().all(|(rel, bytes)| previous.output_digests.get(rel) == Some(&hash_bytes(bytes)))
            }
            Err(_) => false,
        }
}

/// The files of the output directory `dir` (none if it does not exist), by `/`-separated path,
/// without the build record: it records how the output was generated, not what was generated.
pub fn output_files(dir: &Path) -> Result<BTreeMap<String, Vec<u8>>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> Result<()> {
        let entries = fs::read_dir(dir).map_err(|e| Error::io(format!("cannot read {}", dir.display()), e))?;
        for entry in entries {
            let path = entry.map_err(|e| Error::io(format!("cannot read {}", dir.display()), e))?.path();
            if path.is_dir() {
                walk(root, &path, out)?;
                continue;
            }
            let rel = path.strip_prefix(root).expect("under the root");
            let rel: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
            let rel = rel.join("/");
            if rel == BUILD_INFO {
                continue;
            }
            let bytes = fs::read(&path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
            out.insert(rel, bytes);
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    if dir.exists() {
        walk(dir, dir, &mut out)?;
    }
    Ok(out)
}

/// Publishes `files` into `out_dir` atomically: the complete set is staged and validated in
/// `work_dir`, then swapped into place, so a failed generation never leaves a partially updated
/// output behind.
pub fn publish(
    out_dir: &Path,
    work_dir: &Path,
    files: &BTreeMap<String, String>,
    binary_files: &BTreeMap<String, Vec<u8>>,
    info: &BuildInfo,
) -> Result<()> {
    fs::create_dir_all(work_dir).map_err(|e| Error::io(format!("cannot create {}", work_dir.display()), e))?;
    let staging = work_dir.join(format!("staging-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| Error::io(format!("cannot clear {}", staging.display()), e))?;
    }
    let entries =
        files.iter().map(|(k, v)| (k, v.as_bytes())).chain(binary_files.iter().map(|(k, v)| (k, v.as_slice())));
    let mut info = info.clone();
    for (rel, bytes) in entries {
        if rel.starts_with('/') || rel.split('/').any(|c| c == ".." || c.is_empty()) {
            return Err(Error::Environment(format!(
                "refusing to write generated file outside the output directory: {rel}"
            )));
        }
        if bytes.is_empty() {
            return Err(Error::Environment(format!("generated file {rel} is empty")));
        }
        let dest = staging.join(rel);
        fs::create_dir_all(dest.parent().expect("files have a parent"))
            .map_err(|e| Error::io(format!("cannot create {}", staging.display()), e))?;
        fs::write(&dest, bytes).map_err(|e| Error::io(format!("cannot write {}", dest.display()), e))?;
        info.output_digests.insert(rel.clone(), hash_bytes(bytes));
    }
    let mut record = serde_json::to_string_pretty(&info).expect("build info serializes");
    record.push('\n');
    fs::write(staging.join(BUILD_INFO), record).map_err(|e| Error::io("cannot write build-info.json", e))?;
    let previous = work_dir.join(format!("previous-{}", std::process::id()));
    if previous.exists() {
        fs::remove_dir_all(&previous).map_err(|e| Error::io(format!("cannot clear {}", previous.display()), e))?;
    }
    // The output directory belongs to lungo: it is replaced as a whole. A directory lungo did
    // not create is never replaced.
    if out_dir.exists() && !out_dir.join(BUILD_INFO).is_file() {
        let empty = fs::read_dir(out_dir)
            .map_err(|e| Error::io(format!("cannot read {}", out_dir.display()), e))?
            .next()
            .is_none();
        if !empty {
            return Err(Error::Configuration(format!(
                "refusing to replace {}: it is not a directory lungo generated (it has no {BUILD_INFO}); choose an empty or new output directory",
                out_dir.display()
            )));
        }
    }
    let parent = out_dir.parent().expect("the output directory has a parent");
    fs::create_dir_all(parent).map_err(|e| Error::io(format!("cannot create {}", parent.display()), e))?;
    let had_previous = out_dir.exists();
    if had_previous {
        fs::rename(out_dir, &previous).map_err(|e| Error::io(format!("cannot move aside {}", out_dir.display()), e))?;
    }
    if let Err(e) = fs::rename(&staging, out_dir) {
        if had_previous {
            let _ = fs::rename(&previous, out_dir);
        }
        return Err(Error::io(format!("cannot publish {}", out_dir.display()), e));
    }
    if had_previous {
        fs::remove_dir_all(&previous).map_err(|e| Error::io(format!("cannot remove {}", previous.display()), e))?;
    }
    Ok(())
}

/// Source text of the project's own modules, keyed by module name.
pub fn local_sources(project: &Path, success: &Success) -> Result<BTreeMap<String, String>> {
    let mut out = BTreeMap::new();
    for m in &success.module_graph {
        if let Some(src) = &m.source
            && src.origin == PackageOrigin::Root
        {
            let path = project.join(&src.path);
            let text =
                fs::read_to_string(&path).map_err(|e| Error::io(format!("cannot read {}", path.display()), e))?;
            out.insert(m.name.clone(), text);
        }
    }
    Ok(out)
}
