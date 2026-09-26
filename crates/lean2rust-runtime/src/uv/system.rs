//! `Std.Internal.UV.System`, ported from `runtime/uv/system.cpp` and the libuv functions it
//! calls (`uv_os_*`, `uv_cpu_info`, `uv_getrusage`, memory queries, process title, ...).

use super::errno::*;
use super::reactor::reactor;
use super::*;
use std::sync::Mutex;

type Res<T> = Result<T, i32>;

fn io_code(e: std::io::Error) -> i32 {
    uv_code_of_io_error(&e)
}

unsafe fn ok_str(s: &str) -> Obj {
    unsafe { lean_io_result_mk_ok(lean_mk_string(s)) }
}

unsafe fn ok_u64(v: u64) -> Obj {
    unsafe { lean_io_result_mk_ok(lean_box_uint64(v)) }
}

unsafe fn result<T>(r: Res<T>, f: impl FnOnce(T) -> Obj) -> Obj {
    unsafe {
        match r {
            Ok(v) => f(v),
            Err(code) => uv_io_error(code),
        }
    }
}

/// An error result for a string argument containing NUL (`mk_embedded_nul_error`).
unsafe fn embedded_nul_error(s: Obj) -> Obj {
    unsafe {
        #[cfg(unix)]
        let einval = libc::EINVAL as u32;
        #[cfg(windows)]
        let einval = 22u32;
        lean_inc(s);
        lean_io_result_mk_error(crate::io::mk::invalid_argument_file(
            s,
            einval,
            lean_mk_string("string contains NUL bytes"),
        ))
    }
}

// ---------------------------------------------------------------------------------------------
// Process title
// ---------------------------------------------------------------------------------------------

struct Title {
    title: String,
    /// Bytes available for the title (libuv: the memory of the original `argv` strings).
    cap: usize,
}

static TITLE: Mutex<Option<Title>> = Mutex::new(None);

fn with_title<T>(f: impl FnOnce(&mut Title) -> T) -> Res<T> {
    let mut g = TITLE.lock().unwrap_or_else(|p| p.into_inner());
    if g.is_none() {
        *g = Some(initial_title()?);
    }
    Ok(f(g.as_mut().expect("title initialized")))
}

#[cfg(unix)]
fn initial_title() -> Res<Title> {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let cap: usize = args.iter().map(|a| a.len() + 1).sum();
    let title = args.first().map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Title { title, cap })
}

#[cfg(windows)]
fn initial_title() -> Res<Title> {
    use windows_sys::Win32::System::Console::GetConsoleTitleW;
    let mut buf = vec![0u16; 8192];
    let n = unsafe { GetConsoleTitleW(buf.as_mut_ptr(), buf.len() as u32) };
    if n == 0 {
        return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
    }
    Ok(Title { title: String::from_utf16_lossy(&buf[..n as usize]), cap: usize::MAX })
}

pub(crate) unsafe fn get_process_title() -> Obj {
    unsafe {
        let r = with_title(|t| t.title.clone()).and_then(|t| if t.len() >= 512 { Err(UV_ENOBUFS) } else { Ok(t) });
        result(r, |t| ok_str(&t))
    }
}

pub(crate) unsafe fn set_process_title(title: Obj) -> Obj {
    unsafe {
        let Some(bytes) = addr::c_str_bytes(title) else { return embedded_nul_error(title) };
        let bytes = bytes.to_vec();
        let r = with_title(|t| {
            let mut len = bytes.len();
            if len >= t.cap {
                len = t.cap.saturating_sub(1);
            }
            t.title = String::from_utf8_lossy(&bytes[..len]).into_owned();
            platform_set_title(&t.title);
        });
        result(r, |_| lean_io_result_mk_ok(lean_box(0)))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn platform_set_title(title: &str) {
    if let Ok(c) = std::ffi::CString::new(title) {
        unsafe { libc::prctl(libc::PR_SET_NAME, c.as_ptr() as libc::c_ulong, 0, 0, 0) };
    }
}

#[cfg(windows)]
fn platform_set_title(title: &str) {
    let wide: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { windows_sys::Win32::System::Console::SetConsoleTitleW(wide.as_ptr()) };
}

#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
fn platform_set_title(_title: &str) {}

// ---------------------------------------------------------------------------------------------
// Process and host information
// ---------------------------------------------------------------------------------------------

#[cfg(any(target_os = "linux", target_os = "android"))]
fn uptime() -> Res<u64> {
    let mut ts: libc::timespec = unsafe { std::mem::zeroed() };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    Ok(ts.tv_sec as u64)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn uptime() -> Res<u64> {
    let mut tv: libc::timeval = unsafe { std::mem::zeroed() };
    let mut size = size_of::<libc::timeval>();
    let mut mib = [libc::CTL_KERN, libc::KERN_BOOTTIME];
    let r = unsafe {
        libc::sysctl(mib.as_mut_ptr(), 2, &mut tv as *mut _ as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0)
    };
    if r != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    let now = unsafe { libc::time(std::ptr::null_mut()) };
    Ok((now - tv.tv_sec).max(0) as u64)
}

#[cfg(windows)]
fn uptime() -> Res<u64> {
    Ok(unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() } / 1000)
}

pub(crate) unsafe fn uptime_result() -> Obj {
    unsafe { result(uptime(), |v| ok_u64(v)) }
}

pub(crate) unsafe fn getpid() -> Obj {
    unsafe { ok_u64(std::process::id() as u64) }
}

#[cfg(unix)]
fn ppid() -> u64 {
    unsafe { libc::getppid() as u64 }
}

#[cfg(windows)]
fn ppid() -> u64 {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::*;
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return 0;
        }
        let me = std::process::id();
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut parent = 0u64;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                if entry.th32ProcessID == me {
                    parent = entry.th32ParentProcessID as u64;
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        parent
    }
}

pub(crate) unsafe fn getppid() -> Obj {
    unsafe { ok_u64(ppid()) }
}

struct CpuInfo {
    model: String,
    speed: u64,
    times: [u64; 5],
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn cpu_info() -> Res<Vec<CpuInfo>> {
    let stat = std::fs::read_to_string("/proc/stat").map_err(io_code)?;
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let multiplier = if ticks > 0 { 1000 / ticks as u64 } else { 1 };
    let mut cpus: Vec<(u32, [u64; 5])> = Vec::new();
    for line in stat.lines() {
        let Some(rest) = line.strip_prefix("cpu") else { continue };
        let mut fields = rest.split_whitespace();
        let Some(id) = fields.next().and_then(|f| f.parse::<u32>().ok()) else { continue };
        let v: Vec<u64> = fields.map(|f| f.parse().unwrap_or(0)).collect();
        let get = |i: usize| v.get(i).copied().unwrap_or(0) * multiplier;
        // user nice system idle iowait irq
        cpus.push((id, [get(0), get(1), get(2), get(3), get(5)]));
    }
    let cpuinfo = std::fs::read_to_string("/proc/cpuinfo").unwrap_or_default();
    let models: Vec<String> = cpuinfo
        .lines()
        .filter_map(|l| l.strip_prefix("model name").and_then(|r| r.split_once(':')).map(|(_, m)| m.trim().to_owned()))
        .collect();
    Ok(cpus
        .into_iter()
        .enumerate()
        .map(|(i, (id, times))| {
            let speed = std::fs::read_to_string(format!("/sys/devices/system/cpu/cpu{id}/cpufreq/scaling_cur_freq"))
                .ok()
                .and_then(|s| s.trim().parse::<u64>().ok())
                .map(|khz| khz / 1000)
                .unwrap_or(0);
            let model = models.get(i).or(models.last()).cloned().unwrap_or_else(|| "unknown".to_owned());
            CpuInfo { model, speed, times }
        })
        .collect())
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn sysctl_string(name: &str) -> Option<String> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut size = 0usize;
    unsafe {
        if libc::sysctlbyname(c.as_ptr(), std::ptr::null_mut(), &mut size, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        let mut buf = vec![0u8; size];
        if libc::sysctlbyname(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0)
            != 0
        {
            return None;
        }
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..n]).into_owned())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn sysctl_u64(name: &str) -> Option<u64> {
    let c = std::ffi::CString::new(name).ok()?;
    let mut v: u64 = 0;
    let mut size = size_of::<u64>();
    let r = unsafe {
        libc::sysctlbyname(c.as_ptr(), &mut v as *mut u64 as *mut libc::c_void, &mut size, std::ptr::null_mut(), 0)
    };
    (r == 0).then_some(v)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[allow(deprecated)]
fn cpu_info() -> Res<Vec<CpuInfo>> {
    let ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let multiplier = if ticks > 0 { 1000 / ticks as u64 } else { 1 };
    let model = sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "unknown".to_owned());
    let speed = sysctl_u64("hw.cpufrequency").map(|hz| hz / 1_000_000).unwrap_or(0);
    unsafe {
        let mut count: libc::natural_t = 0;
        let mut info: libc::processor_info_array_t = std::ptr::null_mut();
        let mut msg_count: libc::mach_msg_type_number_t = 0;
        let r = libc::host_processor_info(
            libc::mach_host_self(),
            libc::PROCESSOR_CPU_LOAD_INFO,
            &mut count,
            &mut info,
            &mut msg_count,
        );
        if r != libc::KERN_SUCCESS {
            return Err(UV_EINVAL);
        }
        let loads = info as *const libc::processor_cpu_load_info;
        let mut out = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            let t = (*loads.add(i)).cpu_ticks;
            out.push(CpuInfo {
                model: model.clone(),
                speed,
                times: [
                    t[libc::CPU_STATE_USER as usize] as u64 * multiplier,
                    t[libc::CPU_STATE_NICE as usize] as u64 * multiplier,
                    t[libc::CPU_STATE_SYSTEM as usize] as u64 * multiplier,
                    t[libc::CPU_STATE_IDLE as usize] as u64 * multiplier,
                    0,
                ],
            });
        }
        libc::vm_deallocate(
            libc::mach_task_self(),
            info as libc::vm_address_t,
            msg_count as usize * size_of::<libc::integer_t>(),
        );
        Ok(out)
    }
}

#[cfg(windows)]
mod winapi {
    use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};

    pub fn ntdll(name: &[u8]) -> Option<unsafe extern "system" fn() -> isize> {
        let module: Vec<u16> = "ntdll.dll".encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let h = GetModuleHandleW(module.as_ptr());
            if h.is_null() {
                return None;
            }
            GetProcAddress(h, name.as_ptr())
        }
    }

    /// Reads a registry string or DWORD value under `HKEY_LOCAL_MACHINE`.
    pub fn registry(path: &str, value: &str) -> Option<RegValue> {
        use windows_sys::Win32::System::Registry::*;
        let path: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let value: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            let mut key: HKEY = std::ptr::null_mut();
            if RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_QUERY_VALUE, &mut key) != 0 {
                return None;
            }
            let mut ty: u32 = 0;
            let mut buf = vec![0u8; 1024];
            let mut size = buf.len() as u32;
            let r = RegQueryValueExW(key, value.as_ptr(), std::ptr::null(), &mut ty, buf.as_mut_ptr(), &mut size);
            RegCloseKey(key);
            if r != 0 {
                return None;
            }
            match ty {
                REG_DWORD => Some(RegValue::Dword(u32::from_ne_bytes([buf[0], buf[1], buf[2], buf[3]]))),
                REG_SZ => {
                    let wide: Vec<u16> =
                        buf[..size as usize].chunks_exact(2).map(|c| u16::from_ne_bytes([c[0], c[1]])).collect();
                    let n = wide.iter().position(|&c| c == 0).unwrap_or(wide.len());
                    Some(RegValue::Str(String::from_utf16_lossy(&wide[..n])))
                }
                _ => None,
            }
        }
    }

    pub enum RegValue {
        Dword(u32),
        Str(String),
    }
}

#[cfg(windows)]
fn cpu_info() -> Res<Vec<CpuInfo>> {
    use windows_sys::Win32::System::SystemInformation::{GetSystemInfo, SYSTEM_INFO};
    #[repr(C)]
    struct PerfInfo {
        idle: i64,
        kernel: i64,
        user: i64,
        dpc: i64,
        interrupt: i64,
        interrupt_count: u32,
    }
    type NtQuery = unsafe extern "system" fn(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32;
    unsafe {
        let mut si: SYSTEM_INFO = std::mem::zeroed();
        GetSystemInfo(&mut si);
        let n = si.dwNumberOfProcessors as usize;
        let query: NtQuery = match winapi::ntdll(b"NtQuerySystemInformation\0") {
            Some(f) => std::mem::transmute(f),
            None => return Err(UV_ENOSYS),
        };
        let mut perf: Vec<PerfInfo> = (0..n).map(|_| std::mem::zeroed()).collect();
        let mut len = 0u32;
        // SystemProcessorPerformanceInformation
        let status = query(8, perf.as_mut_ptr() as *mut _, (n * size_of::<PerfInfo>()) as u32, &mut len);
        if status != 0 {
            return Err(UV_EIO);
        }
        let mut out = Vec::with_capacity(n);
        for (i, p) in perf.iter().enumerate() {
            let key = format!("HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\{i}");
            let speed = match winapi::registry(&key, "~MHz") {
                Some(winapi::RegValue::Dword(d)) => d as u64,
                _ => 0,
            };
            let model = match winapi::registry(&key, "ProcessorNameString") {
                Some(winapi::RegValue::Str(s)) => s,
                _ => "unknown".to_owned(),
            };
            out.push(CpuInfo {
                model,
                speed,
                times: [
                    p.user as u64 / 10000,
                    0,
                    (p.kernel - p.idle) as u64 / 10000,
                    p.idle as u64 / 10000,
                    p.interrupt as u64 / 10000,
                ],
            });
        }
        Ok(out)
    }
}

pub(crate) unsafe fn cpu_info_result() -> Obj {
    unsafe {
        result(cpu_info(), |cpus| {
            let arr = lean_alloc_array(cpus.len(), cpus.len());
            for (i, c) in cpus.iter().enumerate() {
                let times = lean_alloc_ctor(0, 0, 40);
                for (k, t) in c.times.iter().enumerate() {
                    lean_ctor_set_uint64(times, 8 * k, *t);
                }
                let info = lean_alloc_ctor(0, 2, 8);
                lean_ctor_set(info, 0, lean_mk_string(&c.model));
                lean_ctor_set(info, 1, times);
                lean_ctor_set_uint64(info, 2 * size_of::<Obj>(), c.speed);
                lean_array_set_core(arr, i, info);
            }
            lean_io_result_mk_ok(arr)
        })
    }
}

/// Removes a trailing path separator as libuv's `uv_cwd` and `uv_os_tmpdir` do.
fn strip_trailing_separator(mut s: String) -> String {
    #[cfg(unix)]
    if s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    #[cfg(windows)]
    if s.len() > 1 && s.ends_with('\\') && !(s.len() == 3 && s.as_bytes()[1] == b':') {
        s.pop();
    }
    s
}

pub(crate) unsafe fn cwd() -> Obj {
    unsafe {
        let r = std::env::current_dir()
            .map_err(io_code)
            .map(|p| strip_trailing_separator(p.to_string_lossy().into_owned()));
        result(r, |s| ok_str(&s))
    }
}

pub(crate) unsafe fn chdir(path: Obj) -> Obj {
    unsafe {
        let Some(bytes) = addr::c_str_bytes(path) else { return embedded_nul_error(path) };
        let p = String::from_utf8_lossy(bytes).into_owned();
        match std::env::set_current_dir(&p) {
            Ok(()) => lean_io_result_mk_ok(lean_box(0)),
            Err(e) => lean_io_result_mk_error(uv_error(io_code(e), Some(path))),
        }
    }
}

#[cfg(unix)]
struct Passwd {
    username: String,
    uid: u64,
    gid: u64,
    shell: Option<String>,
    homedir: Option<String>,
}

#[cfg(unix)]
fn passwd_of(uid: libc::uid_t) -> Res<Passwd> {
    let mut buf = vec![0 as libc::c_char; 4096];
    loop {
        let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::passwd = std::ptr::null_mut();
        let r = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut result) };
        if r == libc::ERANGE {
            let len = buf.len() * 2;
            buf.resize(len, 0);
            continue;
        }
        if r != 0 {
            return Err(uv_code_of_os_error(r));
        }
        if result.is_null() {
            return Err(UV_ENOENT);
        }
        let s = |p: *const libc::c_char| -> Option<String> {
            (!p.is_null()).then(|| unsafe { std::ffi::CStr::from_ptr(p) }.to_string_lossy().into_owned())
        };
        return Ok(Passwd {
            username: s(pw.pw_name).unwrap_or_default(),
            uid: pw.pw_uid as u64,
            gid: pw.pw_gid as u64,
            shell: s(pw.pw_shell),
            homedir: s(pw.pw_dir),
        });
    }
}

#[cfg(unix)]
fn homedir() -> Res<String> {
    match std::env::var_os("HOME") {
        Some(h) => Ok(h.to_string_lossy().into_owned()),
        None => passwd_of(unsafe { libc::geteuid() })?.homedir.ok_or(UV_ENOENT),
    }
}

#[cfg(windows)]
fn profile_dir() -> Res<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::TOKEN_READ;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    use windows_sys::Win32::UI::Shell::GetUserProfileDirectoryW;
    unsafe {
        let mut token: HANDLE = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_READ, &mut token) == 0 {
            return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
        }
        let mut buf = vec![0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = GetUserProfileDirectoryW(token, buf.as_mut_ptr(), &mut len);
        let err = std::io::Error::last_os_error();
        CloseHandle(token);
        if ok == 0 {
            return Err(uv_code_of_os_error(err.raw_os_error().unwrap_or(0)));
        }
        let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Ok(String::from_utf16_lossy(&buf[..n]))
    }
}

#[cfg(windows)]
fn homedir() -> Res<String> {
    match std::env::var_os("USERPROFILE") {
        Some(h) => Ok(h.to_string_lossy().into_owned()),
        None => profile_dir(),
    }
}

pub(crate) unsafe fn os_homedir() -> Obj {
    unsafe { result(homedir(), |s| ok_str(&s)) }
}

#[cfg(unix)]
fn tmpdir() -> Res<String> {
    for var in ["TMPDIR", "TMP", "TEMP", "TEMPDIR"] {
        if let Some(v) = std::env::var_os(var) {
            return Ok(strip_trailing_separator(v.to_string_lossy().into_owned()));
        }
    }
    Ok("/tmp".to_owned())
}

#[cfg(windows)]
fn tmpdir() -> Res<String> {
    use windows_sys::Win32::Storage::FileSystem::GetTempPathW;
    let mut buf = vec![0u16; 1024];
    let n = unsafe { GetTempPathW(buf.len() as u32, buf.as_mut_ptr()) };
    if n == 0 {
        return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
    }
    Ok(strip_trailing_separator(String::from_utf16_lossy(&buf[..n as usize])))
}

pub(crate) unsafe fn os_tmpdir() -> Obj {
    unsafe { result(tmpdir(), |s| ok_str(&s)) }
}

unsafe fn opt_str(s: Option<String>) -> Obj {
    unsafe {
        match s {
            Some(s) => some(lean_mk_string(&s)),
            None => none(),
        }
    }
}

pub(crate) unsafe fn os_get_passwd() -> Obj {
    unsafe {
        #[cfg(unix)]
        let r = passwd_of(libc::geteuid()).map(|p| (p.username, Some(p.uid), Some(p.gid), p.shell, p.homedir));
        #[cfg(windows)]
        let r = (|| -> Res<(String, Option<u64>, Option<u64>, Option<String>, Option<String>)> {
            use windows_sys::Win32::System::WindowsProgramming::GetUserNameW;
            let mut buf = vec![0u16; 257];
            let mut len = buf.len() as u32;
            if GetUserNameW(buf.as_mut_ptr(), &mut len) == 0 {
                return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
            }
            let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            let name = String::from_utf16_lossy(&buf[..n]);
            Ok((name, None, None, None, Some(profile_dir()?)))
        })();
        result(r, |(username, uid, gid, shell, homedir)| {
            let info = lean_alloc_ctor(0, 5, 0);
            lean_ctor_set(info, 0, lean_mk_string(&username));
            lean_ctor_set(
                info,
                1,
                match uid {
                    Some(u) => some(lean_box_uint64(u)),
                    None => none(),
                },
            );
            // As Lean's runtime, the group id is reported whenever the user id is.
            lean_ctor_set(
                info,
                2,
                match (uid, gid) {
                    (Some(_), Some(g)) => some(lean_box_uint64(g)),
                    _ => none(),
                },
            );
            lean_ctor_set(info, 3, opt_str(shell));
            lean_ctor_set(info, 4, opt_str(homedir));
            lean_io_result_mk_ok(info)
        })
    }
}

#[cfg(unix)]
fn group(gid: u64) -> Res<Option<(String, u64, Vec<String>)>> {
    let mut buf = vec![0 as libc::c_char; 4096];
    loop {
        let mut gr: libc::group = unsafe { std::mem::zeroed() };
        let mut result: *mut libc::group = std::ptr::null_mut();
        let r = unsafe { libc::getgrgid_r(gid as libc::gid_t, &mut gr, buf.as_mut_ptr(), buf.len(), &mut result) };
        if r == libc::ERANGE {
            let len = buf.len() * 2;
            buf.resize(len, 0);
            continue;
        }
        if r != 0 {
            return Err(uv_code_of_os_error(r));
        }
        if result.is_null() {
            return Ok(None);
        }
        let name = unsafe { std::ffi::CStr::from_ptr(gr.gr_name) }.to_string_lossy().into_owned();
        let mut members = Vec::new();
        let mut p = gr.gr_mem;
        unsafe {
            while !p.is_null() && !(*p).is_null() {
                members.push(std::ffi::CStr::from_ptr(*p).to_string_lossy().into_owned());
                p = p.add(1);
            }
        }
        return Ok(Some((name, gr.gr_gid as u64, members)));
    }
}

#[cfg(windows)]
fn group(_gid: u64) -> Res<Option<(String, u64, Vec<String>)>> {
    Err(UV_ENOTSUP)
}

pub(crate) unsafe fn os_get_group(gid: u64) -> Obj {
    unsafe {
        match group(gid) {
            Ok(None) => lean_io_result_mk_ok(none()),
            Ok(Some((name, gid, members))) => {
                let arr = lean_alloc_array(members.len(), members.len());
                for (i, m) in members.iter().enumerate() {
                    lean_array_set_core(arr, i, lean_mk_string(m));
                }
                let info = lean_alloc_ctor(0, 2, 8);
                lean_ctor_set(info, 0, lean_mk_string(&name));
                lean_ctor_set(info, 1, arr);
                lean_ctor_set_uint64(info, 2 * size_of::<Obj>(), gid);
                lean_io_result_mk_ok(some(info))
            }
            Err(code) => {
                let fname = lean_mk_string("group");
                let e = uv_error(code, Some(fname));
                lean_dec(fname);
                lean_io_result_mk_error(e)
            }
        }
    }
}

pub(crate) unsafe fn os_environ() -> Obj {
    unsafe {
        let vars: Vec<(String, String)> = std::env::vars_os()
            .map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned()))
            .filter(|(k, _)| !k.is_empty() && !k.starts_with('='))
            .collect();
        let arr = lean_alloc_array(vars.len(), vars.len());
        for (i, (k, v)) in vars.iter().enumerate() {
            let pair = lean_alloc_ctor(0, 2, 0);
            lean_ctor_set(pair, 0, lean_mk_string(k));
            lean_ctor_set(pair, 1, lean_mk_string(v));
            lean_array_set_core(arr, i, pair);
        }
        lean_io_result_mk_ok(arr)
    }
}

/// Serializes changes to the process environment, whose C interface is not thread-safe.
static ENV_LOCK: Mutex<()> = Mutex::new(());

pub(crate) unsafe fn os_getenv(name: Obj) -> Obj {
    unsafe {
        let Some(bytes) = addr::c_str_bytes(name) else { return lean_io_result_mk_ok(none()) };
        let key = String::from_utf8_lossy(bytes).into_owned();
        if key.is_empty() || key.contains('=') {
            return lean_io_result_mk_ok(none());
        }
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        match std::env::var_os(&key) {
            Some(v) => lean_io_result_mk_ok(some(lean_mk_string(&v.to_string_lossy()))),
            None => lean_io_result_mk_ok(none()),
        }
    }
}

pub(crate) unsafe fn os_setenv(name: Obj, value: Obj) -> Obj {
    unsafe {
        let Some(k) = addr::c_str_bytes(name) else { return embedded_nul_error(name) };
        let Some(v) = addr::c_str_bytes(value) else { return embedded_nul_error(value) };
        let (k, v) = (String::from_utf8_lossy(k).into_owned(), String::from_utf8_lossy(v).into_owned());
        if k.is_empty() || k.contains('=') {
            return uv_io_error(UV_EINVAL);
        }
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(&k, &v);
        lean_io_result_mk_ok(lean_box(0))
    }
}

pub(crate) unsafe fn os_unsetenv(name: Obj) -> Obj {
    unsafe {
        let Some(k) = addr::c_str_bytes(name) else { return embedded_nul_error(name) };
        let k = String::from_utf8_lossy(k).into_owned();
        if k.is_empty() || k.contains('=') {
            return uv_io_error(UV_EINVAL);
        }
        let _g = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::remove_var(&k);
        lean_io_result_mk_ok(lean_box(0))
    }
}

#[cfg(unix)]
fn hostname() -> Res<String> {
    let mut buf = [0 as libc::c_char; 256];
    if unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) } != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    buf[buf.len() - 1] = 0;
    Ok(unsafe { std::ffi::CStr::from_ptr(buf.as_ptr()) }.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn hostname() -> Res<String> {
    use windows_sys::Win32::Networking::WinSock::GetHostNameW;
    super::sys::init();
    let mut buf = [0u16; 256];
    if unsafe { GetHostNameW(buf.as_mut_ptr(), buf.len() as i32) } != 0 {
        let e = unsafe { windows_sys::Win32::Networking::WinSock::WSAGetLastError() };
        return Err(uv_code_of_os_error(e));
    }
    let n = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    Ok(String::from_utf16_lossy(&buf[..n]))
}

pub(crate) unsafe fn os_gethostname() -> Obj {
    unsafe { result(hostname(), |s| ok_str(&s)) }
}

const UV_PRIORITY_LOW: i64 = 19;
#[cfg_attr(unix, allow(dead_code))]
const UV_PRIORITY_BELOW_NORMAL: i64 = 10;
#[cfg_attr(unix, allow(dead_code))]
const UV_PRIORITY_NORMAL: i64 = 0;
#[cfg_attr(unix, allow(dead_code))]
const UV_PRIORITY_ABOVE_NORMAL: i64 = -7;
#[cfg_attr(unix, allow(dead_code))]
const UV_PRIORITY_HIGH: i64 = -14;
const UV_PRIORITY_HIGHEST: i64 = -20;

#[cfg(unix)]
fn getpriority(pid: u64) -> Res<i64> {
    unsafe {
        *errno_location() = 0;
        let r = libc::getpriority(libc::PRIO_PROCESS as _, pid as libc::id_t);
        if r == -1 && *errno_location() != 0 {
            return Err(uv_code_of_os_error(*errno_location()));
        }
        Ok(r as i64)
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
unsafe fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__errno_location() }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android"))))]
unsafe fn errno_location() -> *mut libc::c_int {
    unsafe { libc::__error() }
}

#[cfg(unix)]
fn setpriority(pid: u64, priority: i64) -> Res<()> {
    if !(UV_PRIORITY_HIGHEST..=UV_PRIORITY_LOW).contains(&priority) {
        return Err(UV_EINVAL);
    }
    let r = unsafe { libc::setpriority(libc::PRIO_PROCESS as _, pid as libc::id_t, priority as libc::c_int) };
    if r != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    Ok(())
}

#[cfg(windows)]
fn with_process<T>(pid: u64, access: u32, f: impl FnOnce(windows_sys::Win32::Foundation::HANDLE) -> Res<T>) -> Res<T> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcess};
    unsafe {
        if pid == 0 {
            return f(GetCurrentProcess());
        }
        let h = OpenProcess(access, 0, pid as u32);
        if h.is_null() {
            return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
        }
        let r = f(h);
        CloseHandle(h);
        r
    }
}

#[cfg(windows)]
fn getpriority(pid: u64) -> Res<i64> {
    use windows_sys::Win32::System::Threading::*;
    with_process(pid, PROCESS_QUERY_LIMITED_INFORMATION, |h| unsafe {
        let class = GetPriorityClass(h);
        if class == 0 {
            return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
        }
        Ok(match class {
            REALTIME_PRIORITY_CLASS => UV_PRIORITY_HIGHEST,
            HIGH_PRIORITY_CLASS => UV_PRIORITY_HIGH,
            ABOVE_NORMAL_PRIORITY_CLASS => UV_PRIORITY_ABOVE_NORMAL,
            NORMAL_PRIORITY_CLASS => UV_PRIORITY_NORMAL,
            BELOW_NORMAL_PRIORITY_CLASS => UV_PRIORITY_BELOW_NORMAL,
            _ => UV_PRIORITY_LOW,
        })
    })
}

#[cfg(windows)]
fn setpriority(pid: u64, priority: i64) -> Res<()> {
    use windows_sys::Win32::System::Threading::*;
    if !(UV_PRIORITY_HIGHEST..=UV_PRIORITY_LOW).contains(&priority) {
        return Err(UV_EINVAL);
    }
    let class = if priority < UV_PRIORITY_HIGH {
        REALTIME_PRIORITY_CLASS
    } else if priority < UV_PRIORITY_ABOVE_NORMAL {
        HIGH_PRIORITY_CLASS
    } else if priority < UV_PRIORITY_NORMAL {
        ABOVE_NORMAL_PRIORITY_CLASS
    } else if priority < UV_PRIORITY_BELOW_NORMAL {
        NORMAL_PRIORITY_CLASS
    } else if priority < UV_PRIORITY_LOW {
        BELOW_NORMAL_PRIORITY_CLASS
    } else {
        IDLE_PRIORITY_CLASS
    };
    with_process(pid, PROCESS_SET_INFORMATION, |h| unsafe {
        if SetPriorityClass(h, class) == 0 {
            return Err(uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0)));
        }
        Ok(())
    })
}

pub(crate) unsafe fn os_getpriority(pid: u64) -> Obj {
    unsafe { result(getpriority(pid), |p| ok_u64(p as u64)) }
}

pub(crate) unsafe fn os_setpriority(pid: u64, priority: u64) -> Obj {
    unsafe { result(setpriority(pid, priority as i64), |_| lean_io_result_mk_ok(lean_box(0))) }
}

#[cfg(unix)]
fn uname() -> Res<[String; 4]> {
    let mut u: libc::utsname = unsafe { std::mem::zeroed() };
    if unsafe { libc::uname(&mut u) } != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    let s = |f: &[libc::c_char]| unsafe { std::ffi::CStr::from_ptr(f.as_ptr()) }.to_string_lossy().into_owned();
    Ok([s(&u.sysname), s(&u.release), s(&u.version), s(&u.machine)])
}

#[cfg(windows)]
fn uname() -> Res<[String; 4]> {
    use windows_sys::Win32::System::SystemInformation::*;
    #[repr(C)]
    struct OsVersionInfo {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    type RtlGetVersion = unsafe extern "system" fn(*mut OsVersionInfo) -> i32;
    unsafe {
        let mut info: OsVersionInfo = std::mem::zeroed();
        info.size = size_of::<OsVersionInfo>() as u32;
        let f: RtlGetVersion = match winapi::ntdll(b"RtlGetVersion\0") {
            Some(f) => std::mem::transmute(f),
            None => return Err(UV_ENOSYS),
        };
        f(&mut info);
        let release = format!("{}.{}.{}", info.major, info.minor, info.build);
        let mut version = match winapi::registry("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion", "ProductName") {
            Some(winapi::RegValue::Str(s)) => s,
            _ => String::new(),
        };
        let csd_len = info.csd.iter().position(|&c| c == 0).unwrap_or(0);
        if csd_len > 0 {
            if !version.is_empty() {
                version.push(' ');
            }
            version.push_str(&String::from_utf16_lossy(&info.csd[..csd_len]));
        }
        let mut si: SYSTEM_INFO = std::mem::zeroed();
        GetNativeSystemInfo(&mut si);
        let machine = match si.Anonymous.Anonymous.wProcessorArchitecture {
            PROCESSOR_ARCHITECTURE_AMD64 => "x86_64".to_owned(),
            PROCESSOR_ARCHITECTURE_IA64 => "ia64".to_owned(),
            PROCESSOR_ARCHITECTURE_INTEL => {
                let level = si.wProcessorLevel.clamp(3, 6);
                format!("i{level}86")
            }
            PROCESSOR_ARCHITECTURE_ARM => "arm".to_owned(),
            PROCESSOR_ARCHITECTURE_ARM64 => "arm64".to_owned(),
            _ => "unknown".to_owned(),
        };
        Ok(["Windows_NT".to_owned(), release, version, machine])
    }
}

pub(crate) unsafe fn os_uname() -> Obj {
    unsafe {
        result(uname(), |[sysname, release, version, machine]| {
            let u = lean_alloc_ctor(0, 4, 0);
            lean_ctor_set(u, 0, lean_mk_string(&sysname));
            lean_ctor_set(u, 1, lean_mk_string(&release));
            lean_ctor_set(u, 2, lean_mk_string(&version));
            lean_ctor_set(u, 3, lean_mk_string(&machine));
            lean_io_result_mk_ok(u)
        })
    }
}

/// A monotonic clock in nanoseconds from an arbitrary point in the past.
pub(crate) unsafe fn hrtime() -> Obj {
    static BASE: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
    let base = BASE.get_or_init(std::time::Instant::now);
    unsafe { ok_u64(base.elapsed().as_nanos() as u64 + 1) }
}

/// `uv_random`: fills a fresh byte array with cryptographically strong random bytes on a
/// worker thread and resolves the returned promise from the event loop.
pub(crate) unsafe fn random(size: u64) -> Obj {
    unsafe {
        if size > 0x7FFF_FFFF {
            return uv_io_error(UV_E2BIG);
        }
        reactor();
        let promise = new_promise();
        let byte_array = lean_alloc_sarray(1, 0, size as usize);
        lean_inc(promise);
        let (p, ba) = (SendObj(promise), SendObj(byte_array));
        std::thread::Builder::new()
            .name("lean-random".into())
            .spawn(move || {
                let (p, ba) = (p, ba);
                let buf = std::slice::from_raw_parts_mut(lean_sarray_cptr(ba.0), size as usize);
                let status = match getrandom::fill(buf) {
                    Ok(()) => 0,
                    Err(e) => match e.raw_os_error() {
                        Some(raw) => uv_code_of_os_error(raw),
                        None => UV_EIO,
                    },
                };
                reactor().defer(Box::new(move || {
                    let (p, ba) = (p, ba);
                    if status < 0 {
                        lean_dec(ba.0);
                        crate::task::promise_resolve(except_err(uv_error(status, None)), p.0);
                    } else {
                        lean_sarray_set_size(ba.0, size as usize);
                        crate::task::promise_resolve(except_ok(ba.0), p.0);
                    }
                    lean_dec(p.0);
                }));
            })
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start a random-number thread: {e}")));
        lean_io_result_mk_ok(promise)
    }
}

#[cfg(unix)]
fn rusage() -> Res<[u64; 16]> {
    let mut u: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut u) } != 0 {
        return Err(io_code(std::io::Error::last_os_error()));
    }
    let ms = |t: libc::timeval| t.tv_sec as u64 * 1000 + t.tv_usec as u64 / 1000;
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let maxrss = u.ru_maxrss as u64 / 1024;
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let maxrss = u.ru_maxrss as u64;
    Ok([
        ms(u.ru_utime),
        ms(u.ru_stime),
        maxrss,
        u.ru_ixrss as u64,
        u.ru_idrss as u64,
        u.ru_isrss as u64,
        u.ru_minflt as u64,
        u.ru_majflt as u64,
        u.ru_nswap as u64,
        u.ru_inblock as u64,
        u.ru_oublock as u64,
        u.ru_msgsnd as u64,
        u.ru_msgrcv as u64,
        u.ru_nsignals as u64,
        u.ru_nvcsw as u64,
        u.ru_nivcsw as u64,
    ])
}

#[cfg(windows)]
fn rusage() -> Res<[u64; 16]> {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows_sys::Win32::System::Threading::{
        GetCurrentProcess, GetProcessIoCounters, GetProcessTimes, IO_COUNTERS,
    };
    let err = || uv_code_of_os_error(std::io::Error::last_os_error().raw_os_error().unwrap_or(0));
    unsafe {
        let h = GetCurrentProcess();
        let (mut c, mut e, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) =
            (std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed(), std::mem::zeroed());
        if GetProcessTimes(h, &mut c, &mut e, &mut k, &mut u) == 0 {
            return Err(err());
        }
        let mut mem: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        mem.cb = size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(h, &mut mem, mem.cb) == 0 {
            return Err(err());
        }
        let mut io: IO_COUNTERS = std::mem::zeroed();
        if GetProcessIoCounters(h, &mut io) == 0 {
            return Err(err());
        }
        let ms = |f: FILETIME| (((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64) / 10_000;
        let mut r = [0u64; 16];
        r[0] = ms(u);
        r[1] = ms(k);
        r[2] = mem.PeakWorkingSetSize as u64 / 1024;
        r[7] = mem.PageFaultCount as u64;
        r[9] = io.ReadOperationCount;
        r[10] = io.WriteOperationCount;
        Ok(r)
    }
}

pub(crate) unsafe fn getrusage() -> Obj {
    unsafe {
        result(rusage(), |v| {
            let r = lean_alloc_ctor(0, 0, 16 * 8);
            for (i, x) in v.iter().enumerate() {
                lean_ctor_set_uint64(r, 8 * i, *x);
            }
            lean_io_result_mk_ok(r)
        })
    }
}

pub(crate) unsafe fn exepath() -> Obj {
    unsafe {
        let r = std::env::current_exe().map_err(io_code).map(|p| p.to_string_lossy().into_owned());
        result(r, |s| ok_str(&s))
    }
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn meminfo(field: &str) -> u64 {
    std::fs::read_to_string("/proc/meminfo")
        .ok()
        .and_then(|s| {
            s.lines()
                .find_map(|l| l.strip_prefix(field))
                .and_then(|rest| rest.trim().trim_end_matches("kB").trim().parse::<u64>().ok())
        })
        .map(|kb| kb * 1024)
        .unwrap_or(0)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn sysinfo() -> Option<libc::sysinfo> {
    let mut info: libc::sysinfo = unsafe { std::mem::zeroed() };
    (unsafe { libc::sysinfo(&mut info) } == 0).then_some(info)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn free_memory() -> u64 {
    let v = meminfo("MemAvailable:");
    if v != 0 {
        return v;
    }
    sysinfo().map(|i| i.freeram as u64 * i.mem_unit as u64).unwrap_or(0)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn total_memory() -> u64 {
    let v = meminfo("MemTotal:");
    if v != 0 {
        return v;
    }
    sysinfo().map(|i| i.totalram as u64 * i.mem_unit as u64).unwrap_or(0)
}

/// The cgroup (v2 or v1) memory limit and current usage of this process, if constrained.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn cgroup_memory() -> Option<(u64, u64)> {
    let cgroups = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let read = |p: String| -> Option<u64> {
        let s = std::fs::read_to_string(p).ok()?;
        let s = s.trim();
        if s == "max" { None } else { s.parse::<u64>().ok().filter(|v| *v < 0x7FFF_FFFF_FFFF_F000) }
    };
    if let Some(path) = cgroups.lines().find_map(|l| l.strip_prefix("0::")) {
        let base = format!("/sys/fs/cgroup{path}");
        let max = read(format!("{base}/memory.max"));
        let high = read(format!("{base}/memory.high"));
        let limit = match (max, high) {
            (Some(a), Some(b)) => a.min(b),
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => return None,
        };
        let current = read(format!("{base}/memory.current")).unwrap_or(0);
        return Some((limit, current));
    }
    let path = cgroups.lines().find_map(|l| {
        let mut parts = l.splitn(3, ':');
        let (_, ctrls, path) = (parts.next()?, parts.next()?, parts.next()?);
        ctrls.split(',').any(|c| c == "memory").then(|| path.to_owned())
    })?;
    let base = format!("/sys/fs/cgroup/memory{path}");
    let hard = read(format!("{base}/memory.limit_in_bytes"));
    let soft = read(format!("{base}/memory.soft_limit_in_bytes"));
    let limit = match (hard, soft) {
        (Some(a), Some(b)) => a.min(b),
        (Some(a), None) | (None, Some(a)) => a,
        (None, None) => return None,
    };
    let current = read(format!("{base}/memory.usage_in_bytes")).unwrap_or(0);
    Some((limit, current))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn constrained_memory() -> u64 {
    cgroup_memory().map(|(l, _)| l).unwrap_or(0)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn available_memory() -> u64 {
    match cgroup_memory() {
        Some((limit, current)) => limit.saturating_sub(current),
        None => free_memory(),
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
#[allow(deprecated)]
fn free_memory() -> u64 {
    unsafe {
        let mut info: libc::vm_statistics = std::mem::zeroed();
        let mut count =
            (size_of::<libc::vm_statistics>() / size_of::<libc::integer_t>()) as libc::mach_msg_type_number_t;
        if libc::host_statistics(
            libc::mach_host_self(),
            libc::HOST_VM_INFO,
            &mut info as *mut _ as libc::host_info_t,
            &mut count,
        ) != libc::KERN_SUCCESS
        {
            return 0;
        }
        info.free_count as u64 * libc::sysconf(libc::_SC_PAGESIZE) as u64
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn total_memory() -> u64 {
    sysctl_u64("hw.memsize").unwrap_or(0)
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn constrained_memory() -> u64 {
    0
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn available_memory() -> u64 {
    free_memory()
}

#[cfg(windows)]
fn memory_status() -> Option<windows_sys::Win32::System::SystemInformation::MEMORYSTATUSEX> {
    use windows_sys::Win32::System::SystemInformation::*;
    unsafe {
        let mut m: MEMORYSTATUSEX = std::mem::zeroed();
        m.dwLength = size_of::<MEMORYSTATUSEX>() as u32;
        (GlobalMemoryStatusEx(&mut m) != 0).then_some(m)
    }
}

#[cfg(windows)]
fn free_memory() -> u64 {
    memory_status().map(|m| m.ullAvailPhys).unwrap_or(0)
}

#[cfg(windows)]
fn total_memory() -> u64 {
    memory_status().map(|m| m.ullTotalPhys).unwrap_or(0)
}

#[cfg(windows)]
fn constrained_memory() -> u64 {
    0
}

#[cfg(windows)]
fn available_memory() -> u64 {
    free_memory()
}

pub(crate) unsafe fn get_free_memory() -> Obj {
    unsafe { ok_u64(free_memory()) }
}

pub(crate) unsafe fn get_total_memory() -> Obj {
    unsafe { ok_u64(total_memory()) }
}

pub(crate) unsafe fn get_constrained_memory() -> Obj {
    unsafe { ok_u64(constrained_memory()) }
}

pub(crate) unsafe fn get_available_memory() -> Obj {
    unsafe { ok_u64(available_memory()) }
}
