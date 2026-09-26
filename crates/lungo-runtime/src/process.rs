//! Child processes and process attributes, ported from Lean's `runtime/process.cpp`.
//!
//! On Unix a child is started with `fork` and `execvp` exactly as Lean's runtime does, so a
//! program that cannot be executed still yields a child process, which reports the failure on
//! its standard error and exits with status 255. On Windows children are created with
//! `CreateProcess` (through the standard library) using Lean's command-line quoting.

#[cfg(unix)]
use crate::io::decode_io_error;
#[cfg(windows)]
use crate::io::io_result_mk_user_error;
use crate::io::wrap_file;
use crate::object::*;

/// `IO.Process.Stdio` constructor indices.
const PIPED: u8 = 0;
#[cfg_attr(unix, allow(dead_code))]
const INHERIT: u8 = 1;
const NUL: u8 = 2;

/// The operating-system identifier of the calling thread (`IO.getTID`).
pub(crate) fn current_thread_id() -> u64 {
    #[cfg(target_vendor = "apple")]
    {
        let mut tid: u64 = 0;
        let r = unsafe { libc::pthread_threadid_np(0, &mut tid) };
        if r != 0 {
            lean_internal_panic("pthread_threadid_np failed");
        }
        tid
    }
    #[cfg(all(unix, not(target_vendor = "apple")))]
    {
        unsafe { libc::syscall(libc::SYS_gettid) as libc::pid_t as u64 }
    }
    #[cfg(windows)]
    {
        unsafe { windows_sys::Win32::System::Threading::GetCurrentThreadId() as u64 }
    }
}

/// The parsed `IO.Process.SpawnArgs`.
struct SpawnArgs {
    stdin: u8,
    stdout: u8,
    stderr: u8,
    cmd: Vec<u8>,
    args: Vec<Vec<u8>>,
    cwd: Option<Vec<u8>>,
    env: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    inherit_env: bool,
    /// Start the child in a new session; Lean's runtime ignores this on Windows.
    #[cfg_attr(windows, allow(dead_code))]
    setsid: bool,
}

unsafe fn string_bytes(s: Obj) -> Vec<u8> {
    unsafe { lean_string_bytes(s).to_vec() }
}

unsafe fn parse_spawn_args(args: Obj) -> SpawnArgs {
    unsafe {
        let p = size_of::<Obj>();
        let cfg = lean_ctor_get(args, 0);
        let stdio = |i: usize| {
            let m = lean_ctor_get_uint8(cfg, i);
            if m > NUL {
                lean_internal_panic(&format!("invalid IO.Process.Stdio value {m}"));
            }
            m
        };
        let arr = lean_ctor_get(args, 2);
        let argv = (0..lean_array_size(arr)).map(|i| string_bytes(lean_array_get_core(arr, i))).collect();
        let cwd_opt = lean_ctor_get(args, 3);
        let cwd = if cwd_opt.is_scalar() { None } else { Some(string_bytes(lean_ctor_get(cwd_opt, 0))) };
        let env_arr = lean_ctor_get(args, 4);
        let env = (0..lean_array_size(env_arr))
            .map(|i| {
                let pair = lean_array_get_core(env_arr, i);
                let value = lean_ctor_get(pair, 1);
                let value = if value.is_scalar() { None } else { Some(string_bytes(lean_ctor_get(value, 0))) };
                (string_bytes(lean_ctor_get(pair, 0)), value)
            })
            .collect();
        SpawnArgs {
            stdin: stdio(0),
            stdout: stdio(1),
            stderr: stdio(2),
            cmd: string_bytes(lean_ctor_get(args, 1)),
            args: argv,
            cwd,
            env,
            inherit_env: lean_ctor_get_uint8(args, 5 * p) != 0,
            setsid: lean_ctor_get_uint8(args, 5 * p + 1) != 0,
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::*;
    use std::ffi::CString;
    use std::fs::File;
    use std::os::fd::FromRawFd;

    /// A C string for a Lean string; like the C runtime, the bytes after an embedded NUL are
    /// not seen by the operating system.
    fn cstring(bytes: &[u8]) -> CString {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        CString::new(&bytes[..end]).expect("no interior NUL")
    }

    fn last_errno() -> i32 {
        std::io::Error::last_os_error().raw_os_error().unwrap_or(libc::EIO)
    }

    /// `setup_stdio` for piped streams: a close-on-exec pipe `(read, write)`.
    fn pipe() -> Result<(i32, i32), i32> {
        let mut fds = [0i32; 2];
        #[cfg(target_vendor = "apple")]
        unsafe {
            if libc::pipe(fds.as_mut_ptr()) == -1 {
                return Err(last_errno());
            }
            if libc::fcntl(fds[0], libc::F_SETFD, libc::FD_CLOEXEC) != 0
                || libc::fcntl(fds[1], libc::F_SETFD, libc::FD_CLOEXEC) != 0
            {
                return Err(last_errno());
            }
        }
        #[cfg(not(target_vendor = "apple"))]
        unsafe {
            if libc::pipe2(fds.as_mut_ptr(), libc::O_CLOEXEC) == -1 {
                return Err(last_errno());
            }
        }
        Ok((fds[0], fds[1]))
    }

    fn setup(mode: u8) -> Result<Option<(i32, i32)>, i32> {
        if mode == PIPED { pipe().map(Some) } else { Ok(None) }
    }

    /// Reports a failure of the forked child and ends it as Lean's runtime does: the message
    /// goes to standard error, then `exit(-1)` flushes the output buffers the child inherited
    /// from the parent (see [`crate::io::flush_stdio_in_forked_child`]). Nothing is allocated.
    unsafe fn child_error(msg: &[u8]) -> ! {
        unsafe {
            libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
            crate::io::flush_stdio_in_forked_child();
            libc::_exit(-1)
        }
    }

    #[cfg(target_vendor = "apple")]
    unsafe fn clear_environment() {
        unsafe { *libc::_NSGetEnviron() = std::ptr::null_mut() };
    }

    #[cfg(not(target_vendor = "apple"))]
    unsafe fn clear_environment() {
        unsafe { libc::clearenv() };
    }

    pub(super) unsafe fn spawn(a: SpawnArgs) -> Obj {
        unsafe {
            let pipes = (|| -> Result<_, i32> { Ok((setup(a.stdin)?, setup(a.stdout)?, setup(a.stderr)?)) })();
            let (stdin_pipe, stdout_pipe, stderr_pipe) = match pipes {
                Ok(p) => p,
                Err(e) => return lean_io_result_mk_error(decode_io_error(e, None)),
            };
            // Everything the child needs is prepared before `fork`.
            let program = cstring(&a.cmd);
            let argv_strings: Vec<CString> =
                std::iter::once(program.clone()).chain(a.args.iter().map(|s| cstring(s))).collect();
            let mut argv: Vec<*const libc::c_char> = argv_strings.iter().map(|s| s.as_ptr()).collect();
            argv.push(std::ptr::null());
            let env: Vec<(CString, Option<CString>)> =
                a.env.iter().map(|(k, v)| (cstring(k), v.as_deref().map(cstring))).collect();
            let cwd = a.cwd.as_deref().map(cstring);
            let mut cwd_error = b"could not change directory to ".to_vec();
            if let Some(c) = &cwd {
                cwd_error.extend_from_slice(c.as_bytes());
            }
            cwd_error.push(b'\n');
            let mut exec_error = b"could not execute external process '".to_vec();
            exec_error.extend_from_slice(program.as_bytes());
            exec_error.extend_from_slice(b"'\n");
            let dev_null = c"/dev/null";

            let pid = libc::fork();
            if pid == 0 {
                if !a.inherit_env {
                    clear_environment();
                }
                for (k, v) in &env {
                    match v {
                        Some(v) => {
                            libc::setenv(k.as_ptr(), v.as_ptr(), 1);
                        }
                        None => {
                            libc::unsetenv(k.as_ptr());
                        }
                    }
                }
                if let Some((r, w)) = stdin_pipe {
                    libc::dup2(r, 0);
                    libc::close(w);
                } else if a.stdin == NUL {
                    let fd = libc::open(dev_null.as_ptr(), libc::O_RDONLY);
                    libc::dup2(fd, 0);
                }
                if let Some((r, w)) = stdout_pipe {
                    libc::dup2(w, 1);
                    libc::close(r);
                } else if a.stdout == NUL {
                    let fd = libc::open(dev_null.as_ptr(), libc::O_WRONLY);
                    libc::dup2(fd, 1);
                }
                if let Some((r, w)) = stderr_pipe {
                    libc::dup2(w, 2);
                    libc::close(r);
                } else if a.stderr == NUL {
                    let fd = libc::open(dev_null.as_ptr(), libc::O_WRONLY);
                    libc::dup2(fd, 2);
                }
                if let Some(c) = &cwd
                    && libc::chdir(c.as_ptr()) < 0
                {
                    child_error(&cwd_error);
                }
                if a.setsid && libc::setsid() < 0 {
                    child_error(b"INTERNAL PANIC: setsid failed\n");
                }
                libc::execvp(argv[0], argv.as_ptr());
                child_error(&exec_error);
            } else if pid == -1 {
                let e = last_errno();
                for p in [stdin_pipe, stdout_pipe, stderr_pipe].into_iter().flatten() {
                    libc::close(p.0);
                    libc::close(p.1);
                }
                return lean_io_result_mk_error(decode_io_error(e, None));
            }

            let mut parent_stdin = lean_box(0);
            let mut parent_stdout = lean_box(0);
            let mut parent_stderr = lean_box(0);
            if let Some((r, w)) = stdin_pipe {
                libc::close(r);
                parent_stdin = wrap_file(File::from_raw_fd(w), false, true);
            }
            if let Some((r, w)) = stdout_pipe {
                libc::close(w);
                parent_stdout = wrap_file(File::from_raw_fd(r), true, false);
            }
            if let Some((r, w)) = stderr_pipe {
                libc::close(w);
                parent_stderr = wrap_file(File::from_raw_fd(r), true, false);
            }
            let p = size_of::<Obj>();
            let child = lean_alloc_ctor(0, 3, 5);
            lean_ctor_set(child, 0, parent_stdin);
            lean_ctor_set(child, 1, parent_stdout);
            lean_ctor_set(child, 2, parent_stderr);
            lean_ctor_set_uint32(child, 3 * p, pid as u32);
            lean_ctor_set_uint8(child, 3 * p + 4, a.setsid as u8);
            lean_io_result_mk_ok(child)
        }
    }

    pub(super) unsafe fn pid_of(child: Obj) -> libc::pid_t {
        unsafe { lean_ctor_get_uint32(child, 3 * size_of::<Obj>()) as libc::pid_t }
    }

    /// The exit code of a terminated child, with bash's `128 + signal` convention.
    fn exit_code(status: i32) -> u32 {
        if libc::WIFEXITED(status) { libc::WEXITSTATUS(status) as u32 } else { 128 + libc::WTERMSIG(status) as u32 }
    }

    pub(super) unsafe fn wait(child: Obj) -> Obj {
        unsafe {
            let mut status = 0;
            if libc::waitpid(pid_of(child), &mut status, 0) == -1 {
                return lean_io_result_mk_error(decode_io_error(last_errno(), None));
            }
            lean_io_result_mk_ok(lean_box_uint32(exit_code(status)))
        }
    }

    pub(super) unsafe fn try_wait(child: Obj) -> Obj {
        unsafe {
            let mut status = 0;
            match libc::waitpid(pid_of(child), &mut status, libc::WNOHANG) {
                -1 => lean_io_result_mk_error(decode_io_error(last_errno(), None)),
                0 => lean_io_result_mk_ok(lean_box(0)),
                _ => {
                    let some = lean_alloc_ctor(1, 1, 0);
                    lean_ctor_set(some, 0, lean_box_uint32(exit_code(status)));
                    lean_io_result_mk_ok(some)
                }
            }
        }
    }

    pub(super) unsafe fn kill(child: Obj) -> Obj {
        unsafe {
            let pid = pid_of(child);
            let setsid = lean_ctor_get_uint8(child, 3 * size_of::<Obj>() + 4) != 0;
            let r = if setsid { libc::killpg(pid, libc::SIGKILL) } else { libc::kill(pid, libc::SIGKILL) };
            if r == -1 {
                return lean_io_result_mk_error(decode_io_error(last_errno(), None));
            }
            lean_io_result_mk_ok(lean_box(0))
        }
    }

    pub(super) unsafe fn take_stdin(child: Obj) -> Obj {
        unsafe {
            let p = size_of::<Obj>();
            let child2 = lean_alloc_ctor(0, 3, 5);
            lean_ctor_set(child2, 0, lean_box(0));
            for i in 1..3 {
                let h = lean_ctor_get(child, i);
                lean_inc(h);
                lean_ctor_set(child2, i, h);
            }
            lean_ctor_set_uint32(child2, 3 * p, lean_ctor_get_uint32(child, 3 * p));
            lean_ctor_set_uint8(child2, 3 * p + 4, lean_ctor_get_uint8(child, 3 * p + 4));
            let stdin = lean_ctor_get(child, 0);
            lean_inc(stdin);
            lean_dec(child);
            let pair = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(pair, 0, stdin);
            lean_ctor_set(pair, 1, child2);
            lean_io_result_mk_ok(pair)
        }
    }

    pub(super) unsafe fn get_current_dir() -> Obj {
        unsafe {
            match std::env::current_dir() {
                Ok(p) => {
                    use std::os::unix::ffi::OsStrExt;
                    lean_io_result_mk_ok(lean_mk_string_from_bytes(p.as_os_str().as_bytes()))
                }
                Err(e) => lean_io_result_mk_error(crate::io::io_error_from_std(&e, None)),
            }
        }
    }

    pub(super) unsafe fn set_current_dir(path: Obj) -> Obj {
        unsafe {
            let c = cstring(lean_string_bytes(path));
            if libc::chdir(c.as_ptr()) == 0 {
                lean_io_result_mk_ok(lean_box(0))
            } else {
                lean_io_result_mk_error(decode_io_error(last_errno(), Some(path)))
            }
        }
    }
}

#[cfg(windows)]
mod imp {
    use super::*;
    use std::ffi::OsString;
    use std::fs::File;
    use std::os::windows::process::CommandExt;
    use std::process::{Child, Command, Stdio};
    use std::sync::Mutex;

    /// The child process of a Lean `Child` object: its fourth field is an external object.
    static CHILD_CLASS: ExternalClass = ExternalClass { finalize: finalize_child, for_each: for_each_child };

    unsafe fn finalize_child(data: *mut ()) {
        drop(unsafe { Box::from_raw(data as *mut Mutex<Child>) });
    }

    unsafe fn for_each_child(_data: *mut (), _f: &mut dyn FnMut(Obj)) {}

    unsafe fn child_of<'a>(child: Obj) -> &'a Mutex<Child> {
        unsafe {
            let ext = lean_ctor_get(child, 3);
            if !std::ptr::eq(lean_get_external_class(ext), &CHILD_CLASS) {
                lean_internal_panic("expected an IO.Process.Child object");
            }
            &*(lean_get_external_data(ext) as *const Mutex<Child>)
        }
    }

    fn lock(m: &Mutex<Child>) -> std::sync::MutexGuard<'_, Child> {
        match m.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        }
    }

    /// A Lean string as the operating system sees it: like the C runtime, which passes C
    /// strings, the bytes after an embedded NUL are not seen.
    fn os(bytes: &[u8]) -> OsString {
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        OsString::from(String::from_utf8_lossy(&bytes[..end]).into_owned())
    }

    /// The system's message for a Windows error code, as Lean's runtime reports it: the text
    /// `FormatMessage` gives, without inserts and without its trailing line break.
    fn system_message(code: u32) -> String {
        use windows_sys::Win32::System::Diagnostics::Debug::{
            FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
        };
        let mut buf = [0u16; 1024];
        let n = unsafe {
            FormatMessageW(
                FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
                std::ptr::null(),
                code,
                0,
                buf.as_mut_ptr(),
                buf.len() as u32,
                std::ptr::null(),
            )
        };
        String::from_utf16_lossy(&buf[..n as usize]).trim_end().to_owned()
    }

    /// The Windows error code of a failed spawn. Lean's runtime calls `CreateProcessW`, which
    /// reports a program it cannot find as `ERROR_FILE_NOT_FOUND`; the standard library looks
    /// the program up itself first and reports that failure without a code.
    fn spawn_error_code(e: &std::io::Error) -> u32 {
        match (e.raw_os_error(), e.kind()) {
            (Some(code), _) => code as u32,
            (None, std::io::ErrorKind::NotFound) => windows_sys::Win32::Foundation::ERROR_FILE_NOT_FOUND,
            (None, _) => lean_internal_panic(&format!("process creation failed without an OS error code: {e}")),
        }
    }

    fn code_of(e: &std::io::Error) -> String {
        match e.raw_os_error() {
            Some(n) => (n as u32).to_string(),
            None => lean_internal_panic(&format!("I/O error without an OS error code: {e}")),
        }
    }

    fn stdio(mode: u8) -> Stdio {
        match mode {
            PIPED => Stdio::piped(),
            INHERIT => Stdio::inherit(),
            _ => Stdio::null(),
        }
    }

    pub(super) unsafe fn spawn(a: SpawnArgs) -> Obj {
        unsafe {
            let mut cmd = Command::new(os(&a.cmd));
            for arg in &a.args {
                // Lean's quoting: every argument is enclosed in quotes, with `"` escaped.
                let mut quoted = String::from("\"");
                for c in String::from_utf8_lossy(arg).chars() {
                    if c == '"' {
                        quoted.push('\\');
                    }
                    quoted.push(c);
                }
                quoted.push('"');
                cmd.raw_arg(quoted);
            }
            if !a.inherit_env {
                cmd.env_clear();
            }
            for (k, v) in &a.env {
                match v {
                    Some(v) => {
                        cmd.env(os(k), os(v));
                    }
                    None => {
                        cmd.env_remove(os(k));
                    }
                }
            }
            if let Some(c) = &a.cwd {
                cmd.current_dir(os(c));
            }
            cmd.stdin(stdio(a.stdin)).stdout(stdio(a.stdout)).stderr(stdio(a.stderr));
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let code = spawn_error_code(&e);
                    return lean_io_result_mk_error(crate::io::mk::other_error(
                        code,
                        lean_mk_string(&system_message(code)),
                    ));
                }
            };
            let to_handle = |f: File, readable: bool| wrap_file(f, readable, !readable);
            let parent_stdin = child
                .stdin
                .take()
                .map_or(lean_box(0), |s| to_handle(File::from(std::os::windows::io::OwnedHandle::from(s)), false));
            let parent_stdout = child
                .stdout
                .take()
                .map_or(lean_box(0), |s| to_handle(File::from(std::os::windows::io::OwnedHandle::from(s)), true));
            let parent_stderr = child
                .stderr
                .take()
                .map_or(lean_box(0), |s| to_handle(File::from(std::os::windows::io::OwnedHandle::from(s)), true));
            let ext = lean_alloc_external(&CHILD_CLASS, Box::into_raw(Box::new(Mutex::new(child))) as *mut ());
            let obj = lean_alloc_ctor(0, 4, 0);
            lean_ctor_set(obj, 0, parent_stdin);
            lean_ctor_set(obj, 1, parent_stdout);
            lean_ctor_set(obj, 2, parent_stderr);
            lean_ctor_set(obj, 3, ext);
            lean_io_result_mk_ok(obj)
        }
    }

    pub(super) unsafe fn wait(child: Obj) -> Obj {
        unsafe {
            match lock(child_of(child)).wait() {
                Ok(status) => lean_io_result_mk_ok(lean_box_uint32(status.code().unwrap_or(0) as u32)),
                Err(e) => io_result_mk_user_error(&code_of(&e)),
            }
        }
    }

    pub(super) unsafe fn try_wait(child: Obj) -> Obj {
        unsafe {
            match lock(child_of(child)).try_wait() {
                Ok(None) => lean_io_result_mk_ok(lean_box(0)),
                Ok(Some(status)) => {
                    let some = lean_alloc_ctor(1, 1, 0);
                    lean_ctor_set(some, 0, lean_box_uint32(status.code().unwrap_or(0) as u32));
                    lean_io_result_mk_ok(some)
                }
                Err(e) => io_result_mk_user_error(&code_of(&e)),
            }
        }
    }

    pub(super) unsafe fn kill(child: Obj) -> Obj {
        unsafe {
            match lock(child_of(child)).kill() {
                Ok(()) => lean_io_result_mk_ok(lean_box(0)),
                Err(e) => io_result_mk_user_error(&code_of(&e)),
            }
        }
    }

    pub(super) unsafe fn pid(child: Obj) -> u32 {
        unsafe { lock(child_of(child)).id() }
    }

    pub(super) unsafe fn take_stdin(child: Obj) -> Obj {
        unsafe {
            let child2 = lean_alloc_ctor(0, 4, 0);
            lean_ctor_set(child2, 0, lean_box(0));
            for i in 1..4 {
                let h = lean_ctor_get(child, i);
                lean_inc(h);
                lean_ctor_set(child2, i, h);
            }
            let stdin = lean_ctor_get(child, 0);
            lean_inc(stdin);
            lean_dec(child);
            let pair = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(pair, 0, stdin);
            lean_ctor_set(pair, 1, child2);
            lean_io_result_mk_ok(pair)
        }
    }

    pub(super) unsafe fn get_current_dir() -> Obj {
        unsafe {
            match std::env::current_dir() {
                Ok(p) => lean_io_result_mk_ok(lean_mk_string(&p.to_string_lossy())),
                Err(e) => io_result_mk_user_error(&code_of(&e)),
            }
        }
    }

    pub(super) unsafe fn set_current_dir(path: Obj) -> Obj {
        unsafe {
            let bytes = lean_string_bytes(path);
            let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
            match std::env::set_current_dir(os(&bytes[..end])) {
                Ok(()) => lean_io_result_mk_ok(lean_box(0)),
                Err(e) => io_result_mk_user_error(&code_of(&e)),
            }
        }
    }
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        /* IO.Process.spawn (args : SpawnArgs) : IO (Child args.toStdioConfig) */
        fn lean_io_process_spawn(args: obj) -> obj {
            let parsed = parse_spawn_args(args);
            lean_dec(args);
            imp::spawn(parsed)
        }

        /* Child.wait {cfg : @& StdioConfig} : @& Child cfg → IO UInt32 */
        fn lean_io_process_child_wait(_cfg: b_obj, child: b_obj) -> obj {
            imp::wait(child)
        }

        /* Child.tryWait {cfg : @& StdioConfig} : @& Child cfg → IO (Option UInt32) */
        fn lean_io_process_child_try_wait(_cfg: b_obj, child: b_obj) -> obj {
            imp::try_wait(child)
        }

        /* Child.kill {cfg : @& StdioConfig} : @& Child cfg → IO Unit */
        fn lean_io_process_child_kill(_cfg: b_obj, child: b_obj) -> obj {
            imp::kill(child)
        }

        /* Child.pid {cfg : @& StdioConfig} : Child cfg → UInt32 */
        fn lean_io_process_child_pid(_cfg: b_obj, child: obj) -> u32 {
            #[cfg(unix)]
            let pid = imp::pid_of(child) as u32;
            #[cfg(windows)]
            let pid = imp::pid(child);
            lean_dec(child);
            pid
        }

        /* Child.takeStdin {cfg : @& StdioConfig} : Child cfg → IO (cfg.stdin.toHandleType × Child _) */
        fn lean_io_process_child_take_stdin(_cfg: b_obj, child: obj) -> obj {
            imp::take_stdin(child)
        }

        /* getPID : BaseIO UInt32 */
        fn lean_io_process_get_pid() -> u32 {
            std::process::id()
        }

        /* getCurrentDir : IO FilePath */
        fn lean_io_process_get_current_dir() -> obj {
            imp::get_current_dir()
        }

        /* setCurrentDir (path : @& FilePath) : IO Unit */
        fn lean_io_process_set_current_dir(path: b_obj) -> obj {
            imp::set_current_dir(path)
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::externs::*;
    use crate::io::externs::{lean_io_prim_handle_flush, lean_io_prim_handle_put_str, lean_io_prim_handle_read};
    use crate::object::*;

    unsafe fn ok(r: Obj) -> Obj {
        unsafe {
            assert!(lean_io_result_is_ok(r), "expected success");
            lean_io_result_take_value(r)
        }
    }

    unsafe fn string_array(items: &[&str]) -> Obj {
        unsafe {
            let a = lean_alloc_array(items.len(), items.len());
            for (i, s) in items.iter().enumerate() {
                lean_array_set_core(a, i, lean_mk_string(s));
            }
            a
        }
    }

    /// Builds `IO.Process.SpawnArgs`.
    unsafe fn spawn_args(
        stdio: [u8; 3],
        cmd: &str,
        args: &[&str],
        cwd: Option<&str>,
        env: &[(&str, Option<&str>)],
        inherit_env: bool,
    ) -> Obj {
        unsafe {
            let p = size_of::<Obj>();
            let cfg = lean_alloc_ctor(0, 0, 3);
            for (i, m) in stdio.iter().enumerate() {
                lean_ctor_set_uint8(cfg, i, *m);
            }
            let cwd = match cwd {
                None => lean_box(0),
                Some(c) => {
                    let o = lean_alloc_ctor(1, 1, 0);
                    lean_ctor_set(o, 0, lean_mk_string(c));
                    o
                }
            };
            let env_arr = lean_alloc_array(env.len(), env.len());
            for (i, (k, v)) in env.iter().enumerate() {
                let value = match v {
                    None => lean_box(0),
                    Some(v) => {
                        let o = lean_alloc_ctor(1, 1, 0);
                        lean_ctor_set(o, 0, lean_mk_string(v));
                        o
                    }
                };
                let pair = lean_alloc_ctor(0, 2, 0);
                lean_ctor_set(pair, 0, lean_mk_string(k));
                lean_ctor_set(pair, 1, value);
                lean_array_set_core(env_arr, i, pair);
            }
            let o = lean_alloc_ctor(0, 5, 2);
            lean_ctor_set(o, 0, cfg);
            lean_ctor_set(o, 1, lean_mk_string(cmd));
            lean_ctor_set(o, 2, string_array(args));
            lean_ctor_set(o, 3, cwd);
            lean_ctor_set(o, 4, env_arr);
            lean_ctor_set_uint8(o, 5 * p, inherit_env as u8);
            lean_ctor_set_uint8(o, 5 * p + 1, 0);
            o
        }
    }

    /// Reads a handle to its end.
    unsafe fn read_all(h: Obj) -> String {
        unsafe {
            let mut out = Vec::new();
            loop {
                let b = ok(lean_io_prim_handle_read(h, 1024));
                let n = lean_sarray_size(b);
                out.extend_from_slice(std::slice::from_raw_parts(lean_sarray_cptr(b), n));
                lean_dec(b);
                if n == 0 {
                    return String::from_utf8(out).unwrap();
                }
            }
        }
    }

    unsafe fn wait(child: Obj) -> u32 {
        unsafe {
            let code = ok(lean_io_process_child_wait(lean_box(0), child));
            lean_unbox_uint32(code)
        }
    }

    // Expected values below were observed from Lean 4.34.1 (`lean --run`) on the same inputs.

    #[test]
    fn piped_output_and_exit_codes() {
        unsafe {
            let a = spawn_args([1, 0, 0], "sh", &["-c", "echo hi; echo err >&2; exit 3"], None, &[], true);
            let c = ok(lean_io_process_spawn(a));
            assert_eq!(read_all(lean_ctor_get(c, 1)), "hi\n");
            assert_eq!(read_all(lean_ctor_get(c, 2)), "err\n");
            assert_eq!(wait(c), 3);
            lean_dec(c);
            let a = spawn_args([1, 1, 1], "sh", &["-c", "kill -9 $$"], None, &[], true);
            let c = ok(lean_io_process_spawn(a));
            assert_eq!(wait(c), 137);
            lean_dec(c);
        }
    }

    #[test]
    fn missing_program_and_directory_fail_in_the_child() {
        unsafe {
            let a = spawn_args([1, 0, 0], "definitely-not-a-program-xyz", &[], None, &[], true);
            let c = ok(lean_io_process_spawn(a));
            assert_eq!(
                read_all(lean_ctor_get(c, 2)),
                "could not execute external process 'definitely-not-a-program-xyz'\n"
            );
            assert_eq!(wait(c), 255);
            lean_dec(c);
            let a = spawn_args([1, 0, 0], "sh", &["-c", "pwd"], Some("/nonexistent-dir-q"), &[], true);
            let c = ok(lean_io_process_spawn(a));
            assert_eq!(read_all(lean_ctor_get(c, 1)), "");
            assert_eq!(read_all(lean_ctor_get(c, 2)), "could not change directory to /nonexistent-dir-q\n");
            assert_eq!(wait(c), 255);
            lean_dec(c);
        }
    }

    #[test]
    fn stdin_environment_and_cwd() {
        unsafe {
            let a = spawn_args(
                [0, 0, 1],
                "sh",
                &["-c", "cat; echo $FOO $HOME; pwd"],
                Some("/"),
                &[("FOO", Some("x")), ("HOME", None)],
                true,
            );
            let c = ok(lean_io_process_spawn(a));
            let pair = ok(lean_io_process_child_take_stdin(lean_box(0), c));
            let stdin = lean_ctor_get(pair, 0);
            let c2 = lean_ctor_get(pair, 1);
            assert!(lean_ctor_get(c2, 0).is_scalar());
            let text = lean_mk_string("in\n");
            ok(lean_io_prim_handle_put_str(stdin, text));
            ok(lean_io_prim_handle_flush(stdin));
            lean_dec(text);
            // Closing the last reference to the pipe signals end of input to `cat`.
            lean_inc(c2);
            lean_dec(pair);
            assert_eq!(read_all(lean_ctor_get(c2, 1)), "in\nx\n/\n");
            assert_eq!(wait(c2), 0);
            lean_dec(c2);
        }
    }

    #[test]
    fn try_wait_kill_and_cleared_environment() {
        unsafe {
            let a = spawn_args([1, 1, 1], "sleep", &["5"], None, &[], true);
            let c = ok(lean_io_process_spawn(a));
            let t = ok(lean_io_process_child_try_wait(lean_box(0), c));
            assert!(t.is_scalar());
            ok(lean_io_process_child_kill(lean_box(0), c));
            assert_eq!(wait(c), 137);
            lean_dec(c);
            let a = spawn_args([1, 0, 1], "/bin/sh", &["-c", "printf abc; echo ${HOME:-unset}"], None, &[], false);
            let c = ok(lean_io_process_spawn(a));
            assert_eq!(read_all(lean_ctor_get(c, 1)), "abcunset\n");
            assert_eq!(wait(c), 0);
            lean_dec(c);
        }
    }

    #[test]
    fn set_current_dir_error_names_the_path() {
        unsafe {
            crate::exports::recording::install();
            let p = lean_mk_string("/nonexistent-q");
            let r = lean_io_process_set_current_dir(p);
            assert!(lean_io_result_is_error(r));
            let e = crate::exports::recording::read(lean_io_result_get_error(r));
            assert_eq!((e.constructor, e.file.as_deref()), ("no_file_or_directory", Some("/nonexistent-q")));
            lean_dec(r);
            lean_dec(p);
        }
    }
}
