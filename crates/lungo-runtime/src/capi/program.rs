//! Program initialization and entry.

use crate::init::{MainFn, io_error_to_string, run_main};
use crate::object::*;
use std::ffi::{CStr, c_char};

unsafe fn text<'a>(s: *const c_char, what: &str) -> &'a str {
    if s.is_null() {
        lean_internal_panic(&format!("{what} is a null pointer"));
    }
    unsafe { CStr::from_ptr(s) }.to_str().unwrap_or_else(|_| lean_internal_panic(&format!("{what} is not UTF-8")))
}

/// Registers the compiled Lean definition the runtime calls as `symbol` (see
/// [`crate::exports`]).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_register_export(symbol: *const c_char, f: *const ()) {
    let symbol = unsafe { text(symbol, "an export symbol") };
    crate::exports::register(symbol, f);
}

/// The value of a successful `IO` initializer `r` (consumed). A failed initializer terminates
/// the process after reporting the uncaught exception, as Lean's generated `main` does.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_check_initializer(decl: *const c_char, r: Obj) -> Obj {
    unsafe {
        if lean_io_result_is_error(r) {
            let decl = text(decl, "an initializer's declaration name");
            let msg = io_error_to_string(lean_io_result_get_error(r));
            lean_internal_panic(&format!(
                "initialization of Lean declaration '{decl}' failed: uncaught exception: {msg}"
            ));
        }
        lean_io_result_take_value(r)
    }
}

/// Ends module initialization (`IO.initializing` becomes false) and starts the task manager,
/// as Lean's generated `main` does after running the module initializers.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_end_initialization() {
    crate::init::end_initialization();
}

/// Runs a Lean program's `main`: `initialize` runs the module initializers (and ends
/// initialization), then `main` runs on a thread with Lean's stack size. `argv` holds `argc`
/// NUL-terminated UTF-8 arguments (without the program name); invalid UTF-8 is replaced, as
/// Lean's runtime does. Returns the exit code.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_run_main(
    initialize: unsafe extern "C" fn(),
    main: *const (),
    takes_args: bool,
    returns_exit_code: bool,
    argc: usize,
    argv: *const *const c_char,
) -> i32 {
    if main.is_null() {
        lean_internal_panic("lungo_run_main: `main` is a null pointer");
    }
    let args: Vec<String> = (0..argc)
        .map(|i| unsafe {
            let a = *argv.add(i);
            if a.is_null() {
                lean_internal_panic("lungo_run_main: an argument is a null pointer");
            }
            CStr::from_ptr(a).to_string_lossy().into_owned()
        })
        .collect();
    let main = unsafe {
        if takes_args {
            MainFn::WithArgs(std::mem::transmute::<*const (), unsafe extern "C" fn(Obj) -> Obj>(main))
        } else {
            MainFn::NoArgs(std::mem::transmute::<*const (), unsafe extern "C" fn() -> Obj>(main))
        }
    };
    run_main(|| unsafe { initialize() }, main, returns_exit_code, args)
}

/// Reports that the host did not register its implementation of the Lean extern `declaration`
/// before the program was initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_panic_host_extern_missing(declaration: *const c_char) -> ! {
    let declaration = unsafe { text(declaration, "an extern's declaration name") };
    lean_internal_panic(&format!(
        "the host implementation of Lean extern '{declaration}' is not registered; register every host extern before using the program"
    ))
}
