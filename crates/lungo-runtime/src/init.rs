//! Support for constants, module initialization, and program entry used by generated code.
//!
//! Nullary compiler declarations are constants. They are evaluated on first use and the value
//! is marked persistent (never reference counted or freed), which is how Lean treats closed
//! terms and how its interpreter treats every constant. Declarations with `[init]` attributes
//! run during module initialization, in module order, exactly once.

use crate::object::*;
use std::sync::OnceLock;

/// A lazily evaluated, persistent object constant.
pub struct LazyObj {
    cell: OnceLock<SendObj>,
}

impl LazyObj {
    pub const fn new() -> Self {
        LazyObj { cell: OnceLock::new() }
    }

    /// The constant's value, computing it with `init` on first use. The result is persistent:
    /// callers may use it without reference counting.
    #[inline]
    pub fn get(&self, init: unsafe fn() -> Obj) -> Obj {
        self.cell
            .get_or_init(|| unsafe {
                let v = init();
                lean_mark_persistent(v);
                SendObj(v)
            })
            .0
    }
}

impl Default for LazyObj {
    fn default() -> Self {
        Self::new()
    }
}

/// A lazily evaluated scalar constant.
pub struct LazyScalar<T: Copy + Send + Sync + 'static> {
    cell: OnceLock<T>,
}

impl<T: Copy + Send + Sync + 'static> LazyScalar<T> {
    pub const fn new() -> Self {
        LazyScalar { cell: OnceLock::new() }
    }

    #[inline]
    pub fn get(&self, init: unsafe fn() -> T) -> T {
        *self.cell.get_or_init(|| unsafe { init() })
    }
}

impl<T: Copy + Send + Sync + 'static> Default for LazyScalar<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// The value of an `initialize x : T ← act` declaration, set once during module initialization.
pub struct InitCell<T: Copy + Send + Sync + 'static> {
    name: &'static str,
    cell: OnceLock<T>,
}

impl<T: Copy + Send + Sync + 'static> InitCell<T> {
    pub const fn new(name: &'static str) -> Self {
        InitCell { name, cell: OnceLock::new() }
    }

    pub fn set(&self, value: T) {
        if self.cell.set(value).is_err() {
            lean_internal_panic(&format!("initializer of '{}' ran twice", self.name));
        }
    }

    #[inline]
    pub fn get(&self) -> T {
        match self.cell.get() {
            Some(v) => *v,
            None => lean_internal_panic(&format!("'{}' was used before its module was initialized", self.name)),
        }
    }
}

/// Object-valued [`InitCell`].
pub type InitObj = InitCell<SendObj>;

impl InitCell<SendObj> {
    pub fn set_obj(&self, value: Obj) {
        unsafe { lean_mark_persistent(value) };
        self.set(SendObj(value));
    }

    #[inline]
    pub fn get_obj(&self) -> Obj {
        self.get().0
    }
}

/// Renders the `IO.Error` value `err` (borrowed) with the compiled `IO.Error.toString`
/// (`lean_io_error_to_string`), as Lean's runtime does.
pub unsafe fn io_error_to_string(err: Obj) -> String {
    unsafe {
        lean_inc(err);
        let s = crate::exports::call1(crate::exports::Export::IoErrorToString, err);
        let out = lean_string_str(s).to_owned();
        lean_dec(s);
        out
    }
}

/// Prints `uncaught exception: <error>` for the failed `IO` result `r` (borrowed), as Lean's
/// `lean_io_result_show_error` does.
pub unsafe fn lean_io_result_show_error(r: Obj) {
    unsafe {
        let msg = io_error_to_string(lean_io_result_get_error(r));
        use std::io::Write;
        crate::io::flush_stdio();
        let _ = writeln!(std::io::stderr(), "uncaught exception: {msg}");
    }
}

/// Handles the result of an `IO` initializer: failures abort initialization.
pub unsafe fn check_initializer(decl: &str, r: Obj) -> Obj {
    unsafe {
        if lean_io_result_is_error(r) {
            let msg = io_error_to_string(lean_io_result_get_error(r));
            lean_dec(r);
            panic!("initialization of Lean declaration '{decl}' failed: uncaught exception: {msg}");
        }
        lean_io_result_take_value(r)
    }
}

/// How a Lean `main` receives its arguments and reports its exit code.
pub enum MainFn {
    /// `main : IO Unit` or `main : IO UInt32`.
    NoArgs(unsafe extern "C" fn() -> Obj),
    /// `main : List String → IO Unit` or `… → IO UInt32`.
    WithArgs(unsafe extern "C" fn(Obj) -> Obj),
}

/// Runs a Lean program's `main`, mirroring the entry point Lean's C backend emits: module
/// initialization on the calling thread, then `main` on a thread with Lean's default stack size
/// (`LEAN_STACK_SIZE_KB` and `LEAN_MAIN_USE_THREAD` are honoured). Returns the process exit code.
pub fn run_main(initialize: fn(), main: MainFn, returns_exit_code: bool, args: Vec<String>) -> i32 {
    #[cfg(windows)]
    unsafe {
        // As Lean's generated `main` does on Windows: no error dialogs, UTF-8 console output.
        windows_sys::Win32::System::Diagnostics::Debug::SetErrorMode(
            windows_sys::Win32::System::Diagnostics::Debug::SEM_FAILCRITICALERRORS,
        );
        windows_sys::Win32::System::Console::SetConsoleOutputCP(windows_sys::Win32::Globalization::CP_UTF8);
    }
    initialize();
    crate::io::mark_end_initialization();
    crate::task::init_task_manager();
    let run = move || -> i32 {
        unsafe {
            let res = match main {
                MainFn::NoArgs(f) => f(),
                MainFn::WithArgs(f) => {
                    let mut list = lean_box(0);
                    for a in args.iter().rev() {
                        let cell = lean_alloc_ctor(1, 2, 0);
                        lean_ctor_set(cell, 0, lean_mk_string(a));
                        lean_ctor_set(cell, 1, list);
                        list = cell;
                    }
                    f(list)
                }
            };
            let code = if lean_io_result_is_ok(res) {
                if returns_exit_code { lean_unbox_uint32(lean_io_result_get_value(res)) as i32 } else { 0 }
            } else {
                lean_io_result_show_error(res);
                1
            };
            lean_dec(res);
            code
        }
    };
    let code = if std::env::var("LEAN_MAIN_USE_THREAD").as_deref() == Ok("0") {
        run()
    } else {
        std::thread::Builder::new()
            .name("lean-main".into())
            .stack_size(crate::task::thread_stack_size())
            .spawn(run)
            .unwrap_or_else(|e| lean_internal_panic(&format!("cannot start the main thread: {e}")))
            .join()
            .unwrap_or_else(|_| lean_internal_panic("the Lean main thread panicked"))
    };
    crate::task::finalize_task_manager();
    crate::io::flush_stdio();
    code
}
