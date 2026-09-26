//! Host-side orchestration for capturing Lean's final LCNF as Bridge IR.

use lean2rust_bir::{FrameError, Module};
use std::env;
use std::error::Error as StdError;
use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const TOOLCHAIN_FILE: &str = include_str!("../worker/lean-toolchain");
const WORKER_FILES: [(&str, &str); 5] = [
    ("lean-toolchain", include_str!("../worker/lean-toolchain")),
    ("lakefile.toml", include_str!("../worker/lakefile.toml")),
    ("Main.lean", include_str!("../worker/Main.lean")),
    (
        "Lean2Rust/BridgeIR.lean",
        include_str!("../worker/Lean2Rust/BridgeIR.lean"),
    ),
    (
        "Lean2Rust/Capture.lean",
        include_str!("../worker/Lean2Rust/Capture.lean"),
    ),
];

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    MissingCacheDirectory,
    MissingProjectFile(PathBuf),
    MissingLakefile(PathBuf),
    UnsupportedToolchain {
        found: String,
    },
    InvalidModuleName(String),
    ProcessFailed {
        program: &'static str,
        status: std::process::ExitStatus,
        stdout: String,
        stderr: String,
    },
    Protocol(FrameError),
    ModuleMismatch {
        requested: String,
        returned: String,
    },
    CorruptWorkerCache(PathBuf),
    InvalidToolchainOutput(&'static str),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::MissingCacheDirectory => {
                f.write_str("OUT_DIR is unset; specify Config::cache_dir for inspection")
            }
            Self::MissingProjectFile(path) => {
                write!(f, "missing Lean project file: {}", path.display())
            }
            Self::MissingLakefile(project) => write!(
                f,
                "Lean project has neither lakefile.toml nor lakefile.lean: {}",
                project.display()
            ),
            Self::UnsupportedToolchain { found } => write!(
                f,
                "Lean toolchain {found:?} is unsupported; this adapter requires {}",
                TOOLCHAIN_FILE.trim()
            ),
            Self::InvalidModuleName(name) => write!(f, "invalid Lean root module name: {name:?}"),
            Self::ProcessFailed {
                program,
                status,
                stdout,
                stderr,
            } => {
                write!(f, "{program} exited with {status}:\n{stdout}{stderr}")
            }
            Self::Protocol(error) => error.fmt(f),
            Self::ModuleMismatch {
                requested,
                returned,
            } => write!(
                f,
                "Lean worker returned module {returned:?} for requested module {requested:?}"
            ),
            Self::CorruptWorkerCache(path) => write!(
                f,
                "worker cache artifact has a mismatched checksum: {}",
                path.display()
            ),
            Self::InvalidToolchainOutput(command) => {
                write!(f, "{command} returned invalid toolchain identity data")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Protocol(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<FrameError> for Error {
    fn from(value: FrameError) -> Self {
        Self::Protocol(value)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// A normal Lake project and the root module to inspect.
pub struct Config {
    project: PathBuf,
    root_module: Option<String>,
    cache_dir: Option<PathBuf>,
}

impl Config {
    pub fn new(project: impl Into<PathBuf>) -> Self {
        Self {
            project: project.into(),
            root_module: None,
            cache_dir: None,
        }
    }

    pub fn root_module(mut self, module: impl Into<String>) -> Self {
        self.root_module = Some(module.into());
        self
    }

    pub fn cache_dir(mut self, directory: impl Into<PathBuf>) -> Self {
        self.cache_dir = Some(directory.into());
        self
    }

    /// Process the root module with Lean and return checked, versioned BIR.
    pub fn inspect(self) -> Result<Module> {
        let project = fs::canonicalize(&self.project)?;
        require_file(&project.join("lean-toolchain"))?;
        require_file(&project.join("lake-manifest.json"))?;
        if !project.join("lakefile.toml").is_file() && !project.join("lakefile.lean").is_file() {
            return Err(Error::MissingLakefile(project));
        }
        let selected_toolchain = fs::read_to_string(project.join("lean-toolchain"))?;
        if selected_toolchain.trim() != TOOLCHAIN_FILE.trim() {
            return Err(Error::UnsupportedToolchain {
                found: selected_toolchain.trim().to_owned(),
            });
        }
        let module = self
            .root_module
            .ok_or_else(|| Error::InvalidModuleName(String::new()))?;
        validate_module_name(&module)?;
        let cache_dir = match self.cache_dir {
            Some(dir) => dir,
            None => env::var_os("OUT_DIR")
                .map(|path| PathBuf::from(path).join("lean2rust"))
                .ok_or(Error::MissingCacheDirectory)?,
        };
        let cache_dir = if cache_dir.is_absolute() {
            cache_dir
        } else {
            env::current_dir()?.join(cache_dir)
        };
        fs::create_dir_all(&cache_dir)?;
        let worker = prepare_worker(&cache_dir, &project)?;
        let dependency_target = format!("+{module}:deps");
        run(
            Command::new("lake")
                .arg("build")
                .arg(&dependency_target)
                .current_dir(&project),
            "lake build",
        )?;
        let bir_dir = cache_dir.join("bir");
        fs::create_dir_all(&bir_dir)?;
        let module_key = blake3::hash(module.as_bytes()).to_hex().to_string();
        let frame_path = bir_dir.join(format!("{module_key}.bir"));
        run(
            Command::new("lake")
                .arg("env")
                .arg(&worker)
                .arg(&module)
                .arg(&frame_path)
                .current_dir(&project),
            "lean2rust-worker",
        )?;
        let parsed = Module::from_frame(&fs::read(frame_path)?)?;
        if parsed.module != module {
            return Err(Error::ModuleMismatch {
                requested: module,
                returned: parsed.module,
            });
        }
        Ok(parsed)
    }
}

fn require_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        Err(Error::MissingProjectFile(path.to_path_buf()))
    }
}

fn validate_module_name(module: &str) -> Result<()> {
    if module.trim().is_empty() || module.chars().any(char::is_control) {
        return Err(Error::InvalidModuleName(module.to_owned()));
    }
    Ok(())
}

fn run(command: &mut Command, program: &'static str) -> Result<Output> {
    let output = command.output()?;
    if !output.status.success() {
        return Err(Error::ProcessFailed {
            program,
            status: output.status,
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(output)
}

fn toolchain_identity(project: &Path) -> Result<(String, PathBuf)> {
    let version = run(
        Command::new("lake")
            .args(["env", "lean", "--version"])
            .current_dir(project),
        "lake env lean --version",
    )?;
    let version = String::from_utf8(version.stdout)
        .map_err(|_| Error::InvalidToolchainOutput("lake env lean --version"))?;
    if version.trim().is_empty() {
        return Err(Error::InvalidToolchainOutput("lake env lean --version"));
    }
    let sysroot = run(
        Command::new("lake")
            .args(["env", "lean", "--print-prefix"])
            .current_dir(project),
        "lake env lean --print-prefix",
    )?;
    let sysroot = String::from_utf8(sysroot.stdout)
        .map_err(|_| Error::InvalidToolchainOutput("lake env lean --print-prefix"))?;
    let sysroot = sysroot.trim();
    if sysroot.is_empty() {
        return Err(Error::InvalidToolchainOutput(
            "lake env lean --print-prefix",
        ));
    }
    let sysroot = fs::canonicalize(sysroot)?;
    Ok((version, sysroot))
}

fn hash_field(hasher: &mut blake3::Hasher, field: &[u8]) {
    hasher.update(&(field.len() as u64).to_le_bytes());
    hasher.update(field);
}

fn prepare_worker(cache_dir: &Path, project: &Path) -> Result<PathBuf> {
    let (version, sysroot) = toolchain_identity(project)?;
    let mut source_hash = blake3::Hasher::new();
    hash_field(&mut source_hash, version.as_bytes());
    hash_field(&mut source_hash, sysroot.as_os_str().as_encoded_bytes());
    hash_field(&mut source_hash, env::consts::OS.as_bytes());
    hash_field(&mut source_hash, env::consts::ARCH.as_bytes());
    for (path, data) in WORKER_FILES {
        hash_field(&mut source_hash, path.as_bytes());
        hash_field(&mut source_hash, data.as_bytes());
    }
    let identity = source_hash.finalize().to_hex().to_string();
    let worker_dir = cache_dir.join("workers").join(identity);
    let binary_name = OsString::from(format!("lean2rust-worker{}", env::consts::EXE_SUFFIX));
    let worker_binary = worker_dir.join(".lake/build/bin").join(binary_name);
    let checksum_file = worker_dir.join("worker.blake3");
    if worker_binary.is_file() && checksum_file.is_file() {
        let actual = blake3::hash(&fs::read(&worker_binary)?)
            .to_hex()
            .to_string();
        if fs::read_to_string(&checksum_file)?.trim() == actual {
            return Ok(worker_binary);
        }
        return Err(Error::CorruptWorkerCache(worker_binary));
    }
    for (path, data) in WORKER_FILES {
        let destination = worker_dir.join(path);
        fs::create_dir_all(destination.parent().expect("worker source has a parent"))?;
        fs::write(destination, data)?;
    }
    run(
        Command::new("lake").arg("build").current_dir(&worker_dir),
        "lake build worker",
    )?;
    require_file(&worker_binary)?;
    let checksum = blake3::hash(&fs::read(&worker_binary)?)
        .to_hex()
        .to_string();
    fs::write(checksum_file, checksum)?;
    Ok(worker_binary)
}
