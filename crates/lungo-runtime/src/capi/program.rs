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

/// Checks, before a program first runs, that the package providing its Lean type `lean_type`
/// (`provider`) was generated for the layout the program was (`expected`): the package reports
/// `actual`. A difference means the two were generated from different definitions of the type, and
/// the program would read the package's values at the wrong layout.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_check_layout(
    lean_type: *const c_char,
    provider: *const c_char,
    expected: *const c_char,
    actual: *const c_char,
) {
    let what = "lungo_check_layout";
    let (lean_type, provider) = unsafe { (text(lean_type, what), text(provider, what)) };
    let (expected, actual) = unsafe { (text(expected, what), text(actual, what)) };
    if expected != actual {
        lean_internal_panic(&format!(
            "{lean_type} of {provider} has another layout than the one this program was generated for (fingerprint {actual}, expected {expected}): regenerate both from the same Lean definition"
        ));
    }
}

/// Reports that the host did not provide `operation` (the Lean extern `declaration`) of the
/// capability `capability` before the program was initialized.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_panic_capability_missing(
    capability: *const c_char,
    operation: *const c_char,
    declaration: *const c_char,
) -> ! {
    let what = "lungo_panic_capability_missing";
    let (capability, operation, declaration) =
        unsafe { (text(capability, what), text(operation, what), text(declaration, what)) };
    lean_internal_panic(&format!(
        "the host does not provide the capability {capability}: its operation {operation} (Lean extern \
         '{declaration}') is not implemented; provide every capability before using the program"
    ))
}
