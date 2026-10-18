//! Publication of generated artifacts and the build record.

use crate::error::{Error, Result};
use crate::fingerprint::hash_bytes;
use crate::{Analysis, BuildKey, Context, lake, relative_path};
use lungo_protocol::{PackageOrigin, Success};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

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
    /// Input files with their content digests: the project's relative to the Lean project
    /// directory, a Lake dependency's as `<package:NAME>/…`, relative to that package's directory.
    pub input_digests: BTreeMap<String, String>,
    /// The Lean project directory, relative to the Cargo package (`.` when it is the package), or
    /// `<package:NAME>` when it is outside the package.
    pub project: String,
    pub link_directives: Vec<String>,
    /// The generated sources, relative to the output directory, with their content digests: an
    /// output is reused only while it still holds exactly these.
    pub output_digests: BTreeMap<String, String>,
    /// The output's platform build products, relative to the output directory: files linked
    /// for the machine that wrote them (the TypeScript binding's `program.wasm`), which differ
    /// between machines building the same sources. Named, not digested — each machine links
    /// its own, and an output is reused while it holds them.
    pub platform_products: Vec<String>,
}

impl BuildInfo {
    pub fn new(
        key: &BuildKey,
        ctx: &Context,
        analysis: &Analysis,
        input_files: &[String],
        link_directives: &[String],
    ) -> Result<Self> {
        let packages = lake::packages(&ctx.project)?;
        let mut input_digests = BTreeMap::new();
        for rel in input_files {
            let p = ctx.project.join(rel);
            let bytes = fs::read(&p).map_err(|e| Error::io(format!("cannot read input {}", p.display()), e))?;
            input_digests.insert(input_key(&ctx.project, &packages, rel)?, hash_bytes(&bytes));
        }
        let project = if ctx.local_prefix.is_empty() {
            ".".to_owned()
        } else if leaves_base(&ctx.local_prefix) {
            format!("<package:{}>", lake::validate_project(&ctx.project)?.name)
        } else {
            ctx.local_prefix.clone()
        };
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
            project,
            link_directives: link_directives.to_vec(),
            output_digests: BTreeMap::new(),
            platform_products: Vec::new(),
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
    pub platform_products: Vec<String>,
}

/// The record of how an output directory was generated.
pub const BUILD_INFO: &str = "build-info.json";

/// The name `build-info.json` records an input by, given its path relative to the Lean project:
/// relative to the project when the project holds it, `<package:NAME>/…` when a Lake dependency
/// does. Recording a path that climbs out of the project would write where the dependencies sit
/// on this machine into the output.
fn input_key(project: &Path, packages: &[(String, PathBuf)], rel: &str) -> Result<String> {
    let path = lexical(&project.join(rel));
    let owner = std::iter::once((None, lexical(project)))
        .chain(packages.iter().map(|(name, dir)| (Some(name.as_str()), lexical(dir))))
        .filter(|(_, dir)| path.starts_with(dir))
        .max_by_key(|(_, dir)| dir.components().count());
    match owner {
        Some((None, _)) => Ok(rel.to_owned()),
        Some((Some(name), dir)) => Ok(format!("<package:{name}>/{}", relative_path(&dir, &path))),
        None => Err(Error::Project(format!(
            "the input {rel} is neither in the Lean project nor in a package its manifest locks"
        ))),
    }
}

/// The file `build-info.json` names `key` (see [`input_key`]), or `None` when it names a
/// package the manifest no longer locks.
fn resolve_input(project: &Path, packages: &[(String, PathBuf)], key: &str) -> Option<PathBuf> {
    match key.strip_prefix("<package:").and_then(|k| k.split_once(">/")) {
        Some((name, rel)) => packages.iter().find(|(n, _)| n == name).map(|(_, dir)| dir.join(rel)),
        None => Some(project.join(key)),
    }
}

/// `path` with `.` and `name/..` removed, without consulting the file system: a dependency may be
/// named through a directory the project's own path climbs out of.
fn lexical(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir if matches!(out.components().next_back(), Some(Component::Normal(_))) => {
                out.pop();
            }
            c => out.push(c),
        }
    }
    out
}

/// Whether the relative path `rel` climbs out of the directory it is relative to.
fn leaves_base(rel: &str) -> bool {
    rel.split(['/', '\\']).next() == Some("..")
}

/// Reads the build record of the output in `out_dir`, if it exists and is intact, resolving its
/// input paths against the Lean project directory `project` and the packages its manifest locks.
pub fn read_build_info(out_dir: &Path, project: &Path) -> Option<PreviousBuild> {
    let info: BuildInfo = serde_json::from_slice(&fs::read(out_dir.join(BUILD_INFO)).ok()?).ok()?;
    let by_package = info.input_digests.keys().any(|k| k.starts_with("<package:"));
    let packages = if by_package { lake::packages(project).ok()? } else { Vec::new() };
    let digests: Vec<(PathBuf, String)> = info
        .input_digests
        .iter()
        .map(|(key, d)| resolve_input(project, &packages, key).map(|p| (p, d.clone())))
        .collect::<Option<_>>()?;
    Some(PreviousBuild {
        build_key: info.build_key,
        inputs: digests.iter().map(|(p, _)| p.to_string_lossy().into_owned()).collect(),
        digests,
        link_directives: info.link_directives,
        output_digests: info.output_digests,
        platform_products: info.platform_products,
    })
}

/// Whether the previous build in `out_dir` is still current: every input has the recorded
/// contents, the directory holds exactly the sources that build generated, and each of its
/// platform products is there. A generated file edited, removed or added by hand makes it
/// stale, so it is generated again.
pub fn still_current(out_dir: &Path, previous: &PreviousBuild) -> bool {
    let inputs = previous.digests.iter().all(|(p, d)| fs::read(p).map(|b| hash_bytes(&b) == *d).unwrap_or(false));
    inputs
        && match output_files(out_dir) {
            Ok(mut files) => {
                previous.platform_products.iter().all(|rel| files.remove(rel).is_some())
                    && files.len() == previous.output_digests.len()
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

/// Publishes the generated sources `files` and the platform products `products` into `out_dir`
/// atomically: the complete set is staged and validated beside `out_dir`, then swapped into
/// place, so a failed generation never leaves a partially updated output behind. Staging beside
/// the output keeps the swap a rename within one file system, wherever the output is.
pub fn publish(
    out_dir: &Path,
    files: &BTreeMap<String, String>,
    products: &BTreeMap<String, Vec<u8>>,
    info: &BuildInfo,
) -> Result<()> {
    let parent = out_dir.parent().expect("the output directory has a parent");
    fs::create_dir_all(parent).map_err(|e| Error::io(format!("cannot create {}", parent.display()), e))?;
    let name = out_dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let staging = parent.join(format!(".{name}.lungo-staging-{}", std::process::id()));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| Error::io(format!("cannot clear {}", staging.display()), e))?;
    }
    let entries = files.iter().map(|(k, v)| (k, v.as_bytes())).chain(products.iter().map(|(k, v)| (k, v.as_slice())));
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
        if products.contains_key(rel) {
            info.platform_products.push(rel.clone());
        } else {
            info.output_digests.insert(rel.clone(), hash_bytes(bytes));
        }
    }
    let mut record = serde_json::to_string_pretty(&info).expect("build info serializes");
    record.push('\n');
    fs::write(staging.join(BUILD_INFO), record).map_err(|e| Error::io("cannot write build-info.json", e))?;
    let previous = parent.join(format!(".{name}.lungo-previous-{}", std::process::id()));
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
            fs::remove_dir_all(&staging).map_err(|e| Error::io(format!("cannot remove {}", staging.display()), e))?;
            return Err(Error::Configuration(format!(
                "refusing to replace {}: it is not a directory lungo generated (it has no {BUILD_INFO}); choose an empty or new output directory",
                out_dir.display()
            )));
        }
    }
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

/// Replaces the platform products of the generated output `out_dir` with `products`, linked on
/// this machine, leaving its sources and build record as they are. Each file is written beside
/// its destination and renamed over it, so a failure never leaves a partial file.
pub fn replace_products(out_dir: &Path, products: &BTreeMap<String, Vec<u8>>) -> Result<()> {
    if !out_dir.join(BUILD_INFO).is_file() {
        return Err(Error::Configuration(format!(
            "{} is not a directory lungo generated (it has no {BUILD_INFO}); run `lungo generate` first",
            out_dir.display()
        )));
    }
    for (rel, bytes) in products {
        if rel.starts_with('/') || rel.split('/').any(|c| c == ".." || c.is_empty()) {
            return Err(Error::Environment(format!("refusing to write a product outside the output directory: {rel}")));
        }
        if bytes.is_empty() {
            return Err(Error::Environment(format!("linked product {rel} is empty")));
        }
        let dest = out_dir.join(rel);
        let temp = out_dir.join(format!(".{rel}.linking-{}", std::process::id()));
        fs::write(&temp, bytes).map_err(|e| Error::io(format!("cannot write {}", temp.display()), e))?;
        fs::rename(&temp, &dest).map_err(|e| Error::io(format!("cannot replace {}", dest.display()), e))?;
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

#[cfg(test)]
mod platform_product_tests {
    use super::*;

    fn record() -> BuildInfo {
        BuildInfo {
            build_key: "k".into(),
            lean_version: "4".into(),
            lean_githash: "g".into(),
            bir_version: 1,
            adapter_version: 1,
            lungo_version: "1".into(),
            runtime_abi: 1,
            worker_identity: "w".into(),
            input_digests: BTreeMap::new(),
            project: "formal".into(),
            link_directives: Vec::new(),
            output_digests: BTreeMap::new(),
            platform_products: Vec::new(),
        }
    }

    /// An empty directory of the test's own, removed when it ends.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("lungo-{name}-{}", std::process::id()));
            if dir.exists() {
                fs::remove_dir_all(&dir).expect("clear the scratch directory");
            }
            fs::create_dir_all(&dir).expect("create the scratch directory");
            Scratch(dir)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn previous(out: &Path) -> PreviousBuild {
        read_build_info(out, out).expect("the record just written")
    }

    /// TEST0138: The record names a platform product and digests only the sources, so a product linked
    /// again on this machine leaves the output current, while one that is missing, or a source
    /// edited, makes it stale.
    #[test]
    fn test0138_a_product_is_recorded_by_name_and_must_be_present() {
        let root = Scratch::new("product-recorded");
        let out = root.path().join("out");
        let files: BTreeMap<String, String> = [("index.js".to_string(), "js".to_string())].into();
        let products: BTreeMap<String, Vec<u8>> = [("program.wasm".to_string(), b"one".to_vec())].into();
        publish(&out, &files, &products, &record()).expect("published");

        let info: BuildInfo = serde_json::from_slice(&fs::read(out.join(BUILD_INFO)).unwrap()).unwrap();
        assert_eq!(info.platform_products, vec!["program.wasm".to_string()]);
        assert_eq!(info.output_digests.keys().collect::<Vec<_>>(), vec!["index.js"]);
        assert!(still_current(&out, &previous(&out)));

        let relinked: BTreeMap<String, Vec<u8>> = [("program.wasm".to_string(), b"two".to_vec())].into();
        replace_products(&out, &relinked).expect("relinked");
        assert_eq!(fs::read(out.join("program.wasm")).unwrap(), b"two");
        assert!(still_current(&out, &previous(&out)), "a relinked product made the output stale");

        fs::remove_file(out.join("program.wasm")).unwrap();
        assert!(!still_current(&out, &previous(&out)), "an output without its product is current");

        replace_products(&out, &relinked).unwrap();
        fs::write(out.join("index.js"), "edited").unwrap();
        assert!(!still_current(&out, &previous(&out)), "an edited source is current");
    }

    /// TEST0139: Linking into a directory lungo did not generate is refused, not done.
    #[test]
    fn test0139_products_are_linked_only_into_a_generated_output() {
        let root = Scratch::new("product-refused");
        let products: BTreeMap<String, Vec<u8>> = [("program.wasm".to_string(), b"x".to_vec())].into();
        assert!(replace_products(root.path(), &products).is_err());
        assert!(!root.path().join("program.wasm").exists());
    }
}

#[cfg(test)]
mod input_path_tests {
    use super::*;

    /// A project whose manifest locks one package beside it, by a `path` requirement, and one
    /// fetched into `.lake/packages`, as capdag's model requires tagged-urn's and lungo's.
    fn layout() -> (PathBuf, Vec<(String, PathBuf)>) {
        let project = PathBuf::from("/w/capdag/formal");
        let packages = vec![
            ("tagged_urn_formal".to_string(), project.join("../../tagged-urn/formal")),
            ("lungo".to_string(), project.join(".lake/packages/lungo")),
        ];
        (project, packages)
    }

    /// TEST0370: build-info.json records a dependency's input by its package, never by a path
    /// that climbs out of the project, and reads every key back to the file it was made from.
    #[test]
    fn test0370_inputs_are_recorded_by_package_not_by_where_it_sits() {
        let (project, packages) = layout();
        let cases = [
            ("CapDAG/Cap.lean", "CapDAG/Cap.lean"),
            ("lakefile.lean", "lakefile.lean"),
            ("../../tagged-urn/formal/TaggedUrn/Urn.lean", "<package:tagged_urn_formal>/TaggedUrn/Urn.lean"),
            ("../../tagged-urn/formal/./lakefile.toml", "<package:tagged_urn_formal>/lakefile.toml"),
            (".lake/packages/lungo/Lungo/Attr.lean", "<package:lungo>/Lungo/Attr.lean"),
        ];
        for (rel, key) in cases {
            assert_eq!(input_key(&project, &packages, rel).unwrap(), key, "{rel}");
            let back = resolve_input(&project, &packages, key).expect("the package is locked");
            assert_eq!(lexical(&back), lexical(&project.join(rel)), "{key} does not name {rel}");
        }
        let outside = input_key(&project, &packages, "../../elsewhere/Other.lean");
        assert!(outside.is_err(), "an input outside the project and its packages was recorded: {outside:?}");
        assert_eq!(resolve_input(&project, &packages, "<package:gone>/X.lean"), None);
    }
}
