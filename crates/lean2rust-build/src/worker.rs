//! The toolchain-specific Lean worker: building it against the project's exact toolchain,
//! caching it by content, and running it as a supervised child process.

use crate::error::{Error, Result};
use crate::fingerprint::{Hasher, hash_bytes};
use crate::toolchain::Toolchain;
use lean2rust_protocol::{PROTOCOL_VERSION, Request, Response};
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Version of the worker adapter this crate is built for; the worker reports its own.
pub const ADAPTER_VERSION: u32 = 1;

/// The worker's Lean sources, distributed with this crate and compiled against each project's
/// toolchain.
pub const WORKER_SOURCES: &[(&str, &str)] = &[
    ("lean-toolchain", include_str!("../../../worker/lean-toolchain")),
    ("lakefile.toml", include_str!("../../../worker/lakefile.toml")),
    ("lake-manifest.json", include_str!("../../../worker/lake-manifest.json")),
    ("Main.lean", include_str!("../../../worker/Main.lean")),
    ("Lean2Rust/BridgeIR.lean", include_str!("../../../worker/Lean2Rust/BridgeIR.lean")),
    ("Lean2Rust/Cbor.lean", include_str!("../../../worker/Lean2Rust/Cbor.lean")),
    ("Lean2Rust/Diagnostics.lean", include_str!("../../../worker/Lean2Rust/Diagnostics.lean")),
    ("Lean2Rust/Driver.lean", include_str!("../../../worker/Lean2Rust/Driver.lean")),
    ("Lean2Rust/Interface.lean", include_str!("../../../worker/Lean2Rust/Interface.lean")),
    ("Lean2Rust/Lake.lean", include_str!("../../../worker/Lean2Rust/Lake.lean")),
    ("Lean2Rust/LCNFAdapter.lean", include_str!("../../../worker/Lean2Rust/LCNFAdapter.lean")),
    ("Lean2Rust/Protocol.lean", include_str!("../../../worker/Lean2Rust/Protocol.lean")),
];

/// Where compiled workers are kept.
#[derive(Debug, Clone)]
pub enum WorkerCache {
    /// A cache shared by all builds of the current user.
    Shared(PathBuf),
    /// A cache private to one build output directory.
    Hermetic(PathBuf),
}

impl WorkerCache {
    pub fn root(&self) -> &Path {
        match self {
            WorkerCache::Shared(p) | WorkerCache::Hermetic(p) => p,
        }
    }
}

/// The per-user default cache: `$XDG_CACHE_HOME/lean2rust/workers`, `~/.cache/lean2rust/workers`,
/// or `%LOCALAPPDATA%\lean2rust\workers` on Windows.
pub fn default_shared_cache() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("LEAN2RUST_CACHE_DIR") {
        return Ok(PathBuf::from(dir).join("workers"));
    }
    if cfg!(windows) {
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            return Ok(PathBuf::from(dir).join("lean2rust").join("workers"));
        }
    } else if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return Ok(PathBuf::from(dir).join("lean2rust").join("workers"));
    } else if let Some(home) = std::env::var_os("HOME") {
        return Ok(PathBuf::from(home).join(".cache").join("lean2rust").join("workers"));
    }
    Err(Error::Environment(
        "cannot determine a cache directory for Lean workers; set LEAN2RUST_CACHE_DIR or use a hermetic worker cache"
            .into(),
    ))
}

#[derive(Debug, Clone)]
pub struct Worker {
    pub binary: PathBuf,
    pub identity: String,
}

#[derive(Serialize, Deserialize, PartialEq, Eq, Debug)]
struct Metadata {
    identity: String,
    lean_version: String,
    lean_githash: String,
    sysroot_identity: String,
    host: String,
    adapter_version: u32,
    protocol_version: u32,
    bir_version: u32,
    binary_blake3: String,
}

/// The content identity of the worker for `toolchain` on `host`.
pub fn identity(toolchain: &Toolchain, host: &str) -> String {
    let mut h = Hasher::new("worker");
    h.str(&toolchain.version)
        .str(&toolchain.githash)
        .str(&toolchain.sysroot_identity)
        .str(host)
        .field(&ADAPTER_VERSION.to_le_bytes())
        .field(&PROTOCOL_VERSION.to_le_bytes())
        .field(&lean2rust_bir::BIR_VERSION.to_le_bytes());
    for (path, text) in WORKER_SOURCES {
        h.str(path).str(text);
    }
    h.finish()
}

fn binary_name() -> String {
    format!("lean2rust-worker{}", std::env::consts::EXE_SUFFIX)
}

/// Removes Lean and Lake environment variables inherited from an enclosing Lake invocation, so
/// they cannot redirect the worker build or run to another toolchain or project.
fn scrub_lean_env(cmd: &mut Command) {
    for var in [
        "LEAN_PATH",
        "LEAN_SRC_PATH",
        "LEAN_SYSROOT",
        "LEAN_GITHASH",
        "LEAN_CC",
        "LAKE",
        "LAKE_HOME",
        "LAKE_PKG_URL_MAP",
        "ELAN_TOOLCHAIN",
    ] {
        cmd.env_remove(var);
    }
}

/// Returns a verified worker for `toolchain`, building it into `cache` when absent or stale.
pub fn prepare(toolchain: &Toolchain, cache: &WorkerCache, host: &str) -> Result<Worker> {
    let root = cache.root();
    fs::create_dir_all(root).map_err(|e| Error::io(format!("cannot create {}", root.display()), e))?;
    let id = identity(toolchain, host);
    let dir = root.join(&id);
    let lock_path = root.join(format!("{id}.lock"));
    let lock = File::create(&lock_path).map_err(|e| Error::io(format!("cannot create {}", lock_path.display()), e))?;
    lock.lock().map_err(|e| Error::io(format!("cannot lock {}", lock_path.display()), e))?;
    let binary = dir.join(".lake").join("build").join("bin").join(binary_name());
    let meta_path = dir.join("metadata.json");
    let expected = |binary_blake3: String| Metadata {
        identity: id.clone(),
        lean_version: toolchain.version.clone(),
        lean_githash: toolchain.githash.clone(),
        sysroot_identity: toolchain.sysroot_identity.clone(),
        host: host.to_owned(),
        adapter_version: ADAPTER_VERSION,
        protocol_version: PROTOCOL_VERSION,
        bir_version: lean2rust_bir::BIR_VERSION,
        binary_blake3,
    };
    if meta_path.is_file() && binary.is_file() {
        let meta: Option<Metadata> = fs::read(&meta_path).ok().and_then(|b| serde_json::from_slice(&b).ok());
        let actual = fs::read(&binary)
            .map(|b| hash_bytes(&b))
            .map_err(|e| Error::io(format!("cannot read {}", binary.display()), e))?;
        if meta.as_ref() == Some(&expected(actual)) {
            return Ok(Worker { binary, identity: id });
        }
    }
    // Absent or stale: rebuild from scratch.
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|e| Error::io(format!("cannot remove stale worker {}", dir.display()), e))?;
    }
    let tmp = root.join(format!("{id}.tmp-{}", std::process::id()));
    if tmp.exists() {
        fs::remove_dir_all(&tmp).map_err(|e| Error::io(format!("cannot remove {}", tmp.display()), e))?;
    }
    for (path, text) in WORKER_SOURCES {
        let dest = tmp.join(path);
        fs::create_dir_all(dest.parent().expect("worker files have parents"))
            .map_err(|e| Error::io(format!("cannot create {}", tmp.display()), e))?;
        fs::write(&dest, text).map_err(|e| Error::io(format!("cannot write {}", dest.display()), e))?;
    }
    let mut cmd = Command::new(&toolchain.lake);
    cmd.arg("build").current_dir(&tmp);
    scrub_lean_env(&mut cmd);
    let out = cmd.output().map_err(|e| Error::io("cannot run lake to build the Lean worker", e))?;
    if !out.status.success() {
        return Err(Error::Command {
            program: "lake build (lean2rust worker)".into(),
            status: out.status.to_string(),
            output: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        });
    }
    let built = tmp.join(".lake").join("build").join("bin").join(binary_name());
    let hash = fs::read(&built)
        .map(|b| hash_bytes(&b))
        .map_err(|e| Error::io(format!("the worker build did not produce {}", built.display()), e))?;
    let meta = serde_json::to_vec_pretty(&expected(hash)).expect("metadata serializes");
    fs::write(tmp.join("metadata.json"), meta).map_err(|e| Error::io("cannot write worker metadata", e))?;
    fs::rename(&tmp, &dir).map_err(|e| Error::io(format!("cannot install worker into {}", dir.display()), e))?;
    Ok(Worker { binary, identity: id })
}

/// Environment variables passed to the worker in hermetic mode.
const HERMETIC_ENV: &[&str] = &[
    "PATH",
    "HOME",
    "USERPROFILE",
    "SystemRoot",
    "SYSTEMROOT",
    "TEMP",
    "TMP",
    "TMPDIR",
    "LANG",
    "LC_ALL",
    "ELAN_HOME",
    "LOCALAPPDATA",
    "APPDATA",
];

pub struct RunOptions<'a> {
    pub project: &'a Path,
    pub scratch: &'a Path,
    /// Wall-clock time after which the worker's process tree is killed.
    pub timeout: Option<Duration>,
    pub hermetic: bool,
    pub limits: Limits,
}

/// Resource limits of the worker's processes.
///
/// The worker is isolated from the build process (a crash or runaway worker cannot take the
/// build down), but it is not a security sandbox: a Lean project runs build-time code with the
/// user's permissions, like a Cargo build script.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// Processor time: per process on Unix (`RLIMIT_CPU`), for the whole process tree on
    /// Windows (job object).
    pub cpu: Option<Duration>,
    /// Memory in bytes: address space per process on Linux (`RLIMIT_AS`), committed memory of
    /// the whole process tree on Windows (job object). Other platforms cannot enforce it.
    pub memory: Option<u64>,
}

impl Limits {
    /// Fails when the platform cannot enforce a requested limit.
    pub fn check_enforceable(&self) -> Result<()> {
        if self.memory.is_some() && !cfg!(any(target_os = "linux", target_os = "android", windows)) {
            return Err(Error::Environment(format!(
                "a worker memory limit cannot be enforced on {}; remove `worker_memory_limit`",
                std::env::consts::OS
            )));
        }
        Ok(())
    }

    fn describe(&self) -> Option<String> {
        let mut parts = Vec::new();
        if let Some(c) = self.cpu {
            parts.push(format!("CPU time {} s", c.as_secs()));
        }
        if let Some(m) = self.memory {
            parts.push(format!("memory {m} bytes"));
        }
        (!parts.is_empty()).then(|| parts.join(", "))
    }
}

/// Applies the environment policy to a command run on behalf of the worker: in hermetic mode
/// only [`HERMETIC_ENV`] is inherited; otherwise inherited Lean and Lake settings are removed.
fn base_env(cmd: &mut Command, hermetic: bool) {
    if hermetic {
        let keep: Vec<(String, std::ffi::OsString)> =
            HERMETIC_ENV.iter().filter_map(|k| std::env::var_os(k).map(|v| (k.to_string(), v))).collect();
        cmd.env_clear();
        for (k, v) in keep {
            cmd.env(k, v);
        }
    } else {
        scrub_lean_env(cmd);
    }
}

/// The project's Lake environment (`LEAN_PATH`, `LEAN_SYSROOT`, the toolchain's library path,
/// ...), as `lake env` reports it. Lake prints each variable as `NAME=value`; a variable Lake
/// unsets is printed with an empty value.
fn lake_environment(toolchain: &Toolchain, opts: &RunOptions) -> Result<Vec<(String, Option<String>)>> {
    let mut cmd = Command::new(&toolchain.lake);
    cmd.arg("env").current_dir(opts.project).stdin(Stdio::null());
    base_env(&mut cmd, opts.hermetic);
    let out = cmd.output().map_err(|e| Error::io("cannot run `lake env`", e))?;
    if !out.status.success() {
        return Err(Error::Command {
            program: "lake env".into(),
            status: out.status.to_string(),
            output: format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)),
        });
    }
    let text =
        String::from_utf8(out.stdout).map_err(|_| Error::Environment("`lake env` printed non-UTF-8 output".into()))?;
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|line| {
            let (k, v) = line
                .split_once('=')
                .ok_or_else(|| Error::Environment(format!("unexpected `lake env` output line {line:?}")))?;
            Ok((k.to_owned(), (!v.is_empty()).then(|| v.to_owned())))
        })
        .collect()
}

/// The worker's process tree: the worker and every process it starts.
///
/// On Unix the worker leads a new process group; on Windows it runs in a job object that ends
/// all its processes when closed. Killing the tree ends processes the worker started too, which
/// would otherwise outlive a killed worker and keep its output pipes open.
struct ProcessTree {
    #[cfg(unix)]
    group: libc::pid_t,
    #[cfg(windows)]
    job: windows_sys::Win32::Foundation::HANDLE,
}

impl ProcessTree {
    #[cfg(unix)]
    fn spawn(cmd: &mut Command, limits: Limits) -> std::io::Result<(std::process::Child, ProcessTree)> {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
        let set = |resource, soft: u64, hard: u64| {
            let r = libc::rlimit { rlim_cur: soft as libc::rlim_t, rlim_max: hard as libc::rlim_t };
            // SAFETY: `setrlimit` is async-signal-safe; `r` lives on this stack frame.
            if unsafe { libc::setrlimit(resource, &r) } == 0 { Ok(()) } else { Err(std::io::Error::last_os_error()) }
        };
        if limits.cpu.is_some() || limits.memory.is_some() {
            // SAFETY: the closure runs between `fork` and `exec` and only calls `setrlimit`.
            unsafe {
                cmd.pre_exec(move || {
                    if let Some(cpu) = limits.cpu {
                        // The soft limit signals SIGXCPU; the hard limit one second later kills.
                        let secs = cpu.as_secs().max(1);
                        set(libc::RLIMIT_CPU, secs, secs + 1)?;
                    }
                    #[cfg(any(target_os = "linux", target_os = "android"))]
                    if let Some(bytes) = limits.memory {
                        set(libc::RLIMIT_AS, bytes, bytes)?;
                    }
                    Ok(())
                });
            }
        }
        let child = cmd.spawn()?;
        let group = child.id() as libc::pid_t;
        Ok((child, ProcessTree { group }))
    }

    #[cfg(unix)]
    fn kill(&self) {
        // Fails with ESRCH once every process of the group has exited, which is the goal.
        unsafe { libc::kill(-self.group, libc::SIGKILL) };
    }

    #[cfg(windows)]
    fn spawn(cmd: &mut Command, requested: Limits) -> std::io::Result<(std::process::Child, ProcessTree)> {
        use std::os::windows::io::AsRawHandle;
        use windows_sys::Win32::System::JobObjects::*;
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let tree = ProcessTree { job };
        let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(cpu) = requested.cpu {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_TIME;
            // In units of 100 ns.
            limits.BasicLimitInformation.PerJobUserTimeLimit = (cpu.as_nanos() / 100).min(i64::MAX as u128) as i64;
        }
        if let Some(bytes) = requested.memory {
            limits.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_JOB_MEMORY;
            limits.JobMemoryLimit = bytes as usize;
        }
        let ok = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &limits as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut child = cmd.spawn()?;
        if unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) } == 0 {
            let e = std::io::Error::last_os_error();
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }
        Ok((child, tree))
    }

    #[cfg(windows)]
    fn kill(&self) {
        unsafe { windows_sys::Win32::System::JobObjects::TerminateJobObject(self.job, 1) };
    }
}

#[cfg(windows)]
impl Drop for ProcessTree {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::Foundation::CloseHandle(self.job) };
    }
}

/// Whether the operating system stopped a process for exceeding its CPU time limit.
fn exceeded_cpu_limit(status: &std::process::ExitStatus) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        matches!(status.signal(), Some(libc::SIGXCPU))
    }
    #[cfg(windows)]
    {
        // Processes of a job that exceeds its time limit end with `ERROR_NOT_ENOUGH_QUOTA`.
        status.code() == Some(windows_sys::Win32::Foundation::ERROR_NOT_ENOUGH_QUOTA as i32)
    }
}

/// Runs the worker on `request` in the project's Lake environment.
///
/// The worker is a direct child process, so the supervision (the timeout in particular) acts
/// on the worker itself rather than on a wrapper that could leave it running.
pub fn run(worker: &Worker, toolchain: &Toolchain, request: &Request, opts: &RunOptions) -> Result<Response> {
    fs::create_dir_all(opts.scratch).map_err(|e| Error::io(format!("cannot create {}", opts.scratch.display()), e))?;
    let req_path = opts.scratch.join("request.l2rf");
    let resp_path = opts.scratch.join("response.l2rf");
    if resp_path.exists() {
        fs::remove_file(&resp_path).map_err(|e| Error::io("cannot remove a stale worker response", e))?;
    }
    let frame = request.to_frame().map_err(|e| Error::Protocol(e.to_string()))?;
    fs::write(&req_path, frame).map_err(|e| Error::io("cannot write the worker request", e))?;
    let lake_env = lake_environment(toolchain, opts)?;
    let mut cmd = Command::new(&worker.binary);
    cmd.arg(&req_path)
        .arg(&resp_path)
        .current_dir(opts.project)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    base_env(&mut cmd, opts.hermetic);
    for (k, v) in lake_env {
        match v {
            Some(v) => cmd.env(k, v),
            None => cmd.env_remove(k),
        };
    }
    opts.limits.check_enforceable()?;
    let (mut child, tree) =
        ProcessTree::spawn(&mut cmd, opts.limits).map_err(|e| Error::io("cannot start the Lean worker", e))?;
    let mut stdout = child.stdout.take().expect("piped");
    let mut stderr = child.stderr.take().expect("piped");
    let out_reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stdout.read_to_end(&mut s);
        s
    });
    let err_reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = stderr.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait().map_err(|e| Error::io("cannot wait for the Lean worker", e))? {
            Some(status) => break status,
            None => {
                if let Some(limit) = opts.timeout
                    && start.elapsed() > limit
                {
                    tree.kill();
                    let _ = child.wait();
                    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
                    let _ = out_reader.join();
                    return Err(Error::WorkerTimeout { seconds: limit.as_secs(), stderr });
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    // The worker owns no processes beyond its own lifetime: stragglers are ended, which also
    // releases the output pipes they inherited.
    tree.kill();
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).into_owned();
    if !status.success() || !resp_path.is_file() {
        let output = format!("{stdout}{stderr}");
        if let (Some(cpu), true) = (opts.limits.cpu, exceeded_cpu_limit(&status)) {
            return Err(Error::WorkerResourceLimit {
                limit: format!("CPU time ({} s)", cpu.as_secs()),
                stderr: output,
            });
        }
        return Err(Error::WorkerCrashed {
            status: status.to_string(),
            stderr: output,
            limits: opts.limits.describe(),
        });
    }
    let bytes = fs::read(&resp_path).map_err(|e| Error::io("cannot read the worker response", e))?;
    let response = Response::from_frame(&bytes).map_err(|e| Error::Protocol(e.to_string()))?;
    if response.toolchain.adapter_version != ADAPTER_VERSION {
        return Err(Error::Protocol(format!(
            "worker adapter version {} does not match {ADAPTER_VERSION}",
            response.toolchain.adapter_version
        )));
    }
    if response.toolchain.bir_version != lean2rust_bir::BIR_VERSION {
        return Err(Error::Protocol(format!(
            "worker produces Bridge IR {} but lean2rust implements {}",
            response.toolchain.bir_version,
            lean2rust_bir::BIR_VERSION
        )));
    }
    if response.toolchain.lean_githash != toolchain.githash {
        return Err(Error::Protocol(format!(
            "worker reports Lean commit {} but the resolved toolchain is {}",
            response.toolchain.lean_githash, toolchain.githash
        )));
    }
    Ok(response)
}

#[cfg(all(test, unix))]
mod tests {
    //! Gate: worker failures of any kind are reported as errors of the build; they never take
    //! down the process running the build. The worker is replaced by scripts that misbehave.

    use super::*;
    use crate::toolchain::{Toolchain, ToolchainPolicy};
    use crate::{Config, Environment};
    use std::os::unix::fs::PermissionsExt;

    struct Setup {
        dir: PathBuf,
        toolchain: Toolchain,
        request: Request,
    }

    fn setup(name: &str) -> Setup {
        let dir = std::env::temp_dir().join(format!("lean2rust-worker-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let project = dir.join("project");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("lean-toolchain"), "leanprover/lean4:v4.34.1\n").unwrap();
        fs::write(project.join("lakefile.toml"), "name = \"p\"\n\n[[lean_lib]]\nname = \"P\"\n").unwrap();
        fs::write(project.join("P.lean"), "def x := 1\n").unwrap();
        fs::write(
            project.join("lake-manifest.json"),
            r#"{"version": "1.2.0", "packagesDir": ".lake/packages", "packages": [], "name": "p", "lakeDir": ".lake", "fixedToolchain": false}"#,
        )
        .unwrap();
        let toolchain = Toolchain::resolve_pin("leanprover/lean4:v4.34.1", None, ToolchainPolicy::Strict).unwrap();
        let env = Environment::native(dir.clone(), dir.join("out"), dir.join("work"));
        let cfg = Config::new(&project).root_module("P");
        let ctx = cfg.context(&env).unwrap();
        let request = cfg.request(&ctx, &env);
        Setup { dir, toolchain, request }
    }

    fn fake_worker(s: &Setup, script: &str) -> Worker {
        let binary = s.dir.join("fake-worker");
        fs::write(&binary, format!("#!/bin/sh\n{script}\n")).unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
        Worker { binary, identity: "fake".into() }
    }

    fn run_fake(s: &Setup, script: &str, timeout: Option<Duration>) -> Result<Response> {
        run_limited(s, script, timeout, Limits::default())
    }

    fn run_limited(s: &Setup, script: &str, timeout: Option<Duration>, limits: Limits) -> Result<Response> {
        let worker = fake_worker(s, script);
        run(
            &worker,
            &s.toolchain,
            &s.request,
            &RunOptions {
                project: &s.dir.join("project"),
                scratch: &s.dir.join("work"),
                timeout,
                hermetic: false,
                limits,
            },
        )
    }

    #[test]
    fn crashing_worker_is_an_error() {
        let s = setup("crash");
        let err = run_fake(&s, "echo 'about to crash' >&2\nkill -SEGV $$", None).unwrap_err();
        match err {
            ref e @ Error::WorkerCrashed { ref stderr, .. } => {
                assert!(stderr.contains("about to crash"), "{stderr}");
                assert!(e.to_string().starts_with("error[L2R0303]: "), "{e}");
            }
            other => panic!("expected a crash report, got {other:?}"),
        }
    }

    #[test]
    fn worker_exiting_without_a_response_is_an_error() {
        let s = setup("silent");
        assert!(matches!(run_fake(&s, "exit 0", None), Err(Error::WorkerCrashed { .. })));
        assert!(matches!(run_fake(&s, "exit 3", None), Err(Error::WorkerCrashed { .. })));
    }

    #[test]
    fn malformed_responses_are_protocol_errors() {
        let s = setup("garbage");
        let err = run_fake(&s, "printf 'not a frame' > \"$2\"", None).unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
        // A well-formed header with a truncated payload.
        let err = run_fake(
            &s,
            "printf 'L2RF\\002\\001\\000\\000\\000\\377\\000\\000\\000\\000\\000\\000\\000' > \"$2\"",
            None,
        )
        .unwrap_err();
        assert!(matches!(err, Error::Protocol(_)), "{err:?}");
    }

    #[test]
    fn hung_worker_is_killed_at_the_timeout() {
        let s = setup("hang");
        let start = Instant::now();
        let err = run_fake(&s, "echo started >&2\nsleep 60", Some(Duration::from_secs(1))).unwrap_err();
        let elapsed = start.elapsed();
        match err {
            Error::WorkerTimeout { seconds, stderr } => {
                assert_eq!(seconds, 1);
                assert!(stderr.contains("started"), "{stderr}");
            }
            other => panic!("expected a timeout, got {other:?}"),
        }
        assert!(elapsed < Duration::from_secs(20), "the timeout took {elapsed:?}: the worker was not killed");
    }

    #[test]
    fn cpu_limit_stops_a_spinning_worker() {
        let s = setup("cpu");
        let limits = Limits { cpu: Some(Duration::from_secs(1)), memory: None };
        let start = Instant::now();
        let err = run_limited(&s, "while :; do :; done", Some(Duration::from_secs(60)), limits).unwrap_err();
        assert!(matches!(&err, Error::WorkerResourceLimit { limit, .. } if limit.contains("CPU")), "{err:?}");
        assert!(start.elapsed() < Duration::from_secs(30), "the limit took {:?}", start.elapsed());
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn memory_limit_bounds_the_worker() {
        let s = setup("memory");
        let limits = Limits { cpu: None, memory: Some(512 << 20) };
        // Allocating 2 GiB fails under a 512 MiB address-space limit (and succeeds without it).
        let script = "python3 -c 'bytearray(2 << 30)'";
        let err = run_limited(&s, script, Some(Duration::from_secs(60)), limits).unwrap_err();
        match err {
            Error::WorkerCrashed { stderr, limits, .. } => {
                assert!(stderr.contains("MemoryError"), "the allocation was not refused:\n{stderr}");
                assert_eq!(limits.as_deref(), Some("memory 536870912 bytes"));
            }
            other => panic!("expected the allocation to fail, got {other:?}"),
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    #[test]
    fn unenforceable_memory_limits_are_rejected() {
        let s = setup("no-memory-limit");
        let limits = Limits { cpu: None, memory: Some(1 << 30) };
        let err = run_limited(&s, "exit 0", None, limits).unwrap_err();
        assert!(matches!(&err, Error::Environment(m) if m.contains("cannot be enforced")), "{err:?}");
    }
}
