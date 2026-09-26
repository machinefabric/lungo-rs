//! Lean-level panics (`panic!`, `sorry` at runtime), ported from `runtime/object.cpp`.

use crate::object::*;
use std::sync::atomic::{AtomicBool, Ordering};

static EXIT_ON_PANIC: AtomicBool = AtomicBool::new(false);
static PANIC_MESSAGES: AtomicBool = AtomicBool::new(true);

/// Makes Lean panics terminate the process (exit code 1) after reporting.
pub fn set_exit_on_panic(flag: bool) {
    EXIT_ON_PANIC.store(flag, Ordering::Relaxed);
}

/// Enables or disables panic messages.
pub fn set_panic_messages(flag: bool) {
    PANIC_MESSAGES.store(flag, Ordering::Relaxed);
}

fn abort_on_panic_requested() -> bool {
    std::env::var_os("LEAN_ABORT_ON_PANIC").is_some()
}

fn eprintln(line: &str) {
    if EXIT_ON_PANIC.load(Ordering::Relaxed) || abort_on_panic_requested() {
        // The process is about to terminate: bypass Lean's stderr redirection.
        use std::io::Write;
        let _ = writeln!(std::io::stderr(), "{line}");
    } else {
        crate::io::io_eprintln(line);
    }
}

/// Reports a Lean panic with `msg` and continues, unless configured to terminate.
pub fn lean_panic(msg: &str) {
    if PANIC_MESSAGES.load(Ordering::Relaxed) {
        eprintln(msg);
        if std::env::var("LEAN_BACKTRACE").map(|v| v != "0").unwrap_or(true) {
            eprintln("backtrace:");
            let trace = std::backtrace::Backtrace::force_capture().to_string();
            for line in trace.lines() {
                eprintln(line);
            }
        }
    }
    if abort_on_panic_requested() {
        std::process::abort();
    }
    if EXIT_ON_PANIC.load(Ordering::Relaxed) {
        std::process::exit(1);
    }
}

/// `panicCore`: reports `msg` (owned) and returns `default_val` (owned).
pub unsafe fn lean_panic_fn(default_val: Obj, msg: Obj) -> Obj {
    unsafe {
        lean_panic(lean_string_str(msg));
        lean_dec(msg);
    }
    default_val
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_panic_fn_borrowed(default_val: b_obj, msg: obj) -> obj {
            lean_inc(default_val);
            lean_panic_fn(default_val, msg)
        }

        fn lean_sorry(_unit: u8) -> obj {
            lean_internal_panic("executed 'sorry'")
        }
    }
}
