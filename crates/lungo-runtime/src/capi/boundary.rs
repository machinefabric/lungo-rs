//! The boundary between generated programs and the language bindings, over the
//! [wire format](crate::wire).
//!
//! A binding calls an exported Lean function through the program's generated
//! `<prefix>call_<function>`, which decodes the arguments from the input with a *call context*
//! (`lungo_call_*`), calls the compiled function, and encodes the result. Lean calls a host
//! implementation of an extern through a generated adapter, which encodes the arguments with a
//! *host call* (`lungo_hostcall_*`) and decodes the host's result. Values the host holds without
//! copying (opaque values, closures, `IO.Error`s) are handles: the arguments of a call lend
//! theirs, a result transfers its handles to the receiver.
//!
//! Malformed input from a binding is reported to it (status 1 and a message), not trusted. A
//! host function that returns malformed data, or fails where Lean has no way to observe the
//! failure, terminates the process with a message naming the Lean declaration.

use crate::object::*;
use crate::wire::{self, Buffer, Handles, Host, Reader, Type, TypeTable, WireError, result};
use std::ffi::{CStr, c_char};

/// Status of a call: the output holds the encoded result.
pub const OK: i32 = 0;
/// Status of a call: the input was malformed; the output holds a UTF-8 message.
pub const MALFORMED: i32 = 1;

/// Loads a program's type table. The table is generated with the program: an invalid one is an
/// internal error of the generator.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_types_load(bytes: *const u8, len: usize) -> *const TypeTable {
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    match TypeTable::decode(bytes) {
        Ok(t) => Box::into_raw(Box::new(t)),
        Err(e) => lean_internal_panic(&format!("a generated type table is invalid: {e}")),
    }
}

unsafe fn table(types: *const TypeTable) -> &'static TypeTable {
    if types.is_null() {
        lean_internal_panic("a lungo type table is a null pointer");
    }
    unsafe { &*types }
}

/// A type expression of generated code; a malformed one is an internal error of the generator.
unsafe fn generated_type(bytes: *const u8, len: usize) -> Type {
    let bytes = unsafe { std::slice::from_raw_parts(bytes, len) };
    wire::parse_type(bytes).unwrap_or_else(|e| lean_internal_panic(&format!("a generated type expression is invalid: {e}")))
}

/// The type arguments at the start of an input: a count and that many type expressions, each
/// valid in the program's table and without parameters.
fn type_args(t: &TypeTable, r: &mut Reader) -> Result<Vec<Type>, WireError> {
    let n = r.u32()?;
    let mut args = Vec::new();
    for _ in 0..n {
        let ty = r.ty()?;
        t.check_type(&ty, 0)?;
        args.push(ty);
    }
    Ok(args)
}

/// `lungo_call`: decoding a call's arguments and encoding its result.
pub struct Call {
    table: &'static TypeTable,
    input: Vec<u8>,
    pos: usize,
    type_args: Vec<Type>,
    error: Option<String>,
    output: Vec<u8>,
}

impl Call {
    fn fail(&mut self, e: WireError) {
        if self.error.is_none() {
            self.error = Some(e.0);
        }
    }

    /// `ty` of generated code, instantiated with the call's type arguments.
    fn instantiate(&self, ty: &Type) -> Type {
        ty.substitute(&self.type_args)
            .unwrap_or_else(|e| lean_internal_panic(&format!("a generated signature does not match its call: {e}")))
    }
}

/// Starts a call with the input `input`: the call's type arguments, then its arguments.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_begin(types: *const TypeTable, input: *const u8, len: usize) -> *mut Call {
    let table = unsafe { table(types) };
    let input = if len == 0 { Vec::new() } else { unsafe { std::slice::from_raw_parts(input, len) }.to_vec() };
    let mut r = Reader::new(&input);
    let header = type_args(table, &mut r).map(|args| (args, input.len() - r.remaining()));
    let mut call = Call { table, input, pos: 0, type_args: Vec::new(), error: None, output: Vec::new() };
    match header {
        Ok((args, pos)) => {
            call.type_args = args;
            call.pos = pos;
        }
        Err(e) => call.fail(e),
    }
    Box::into_raw(Box::new(call))
}

/// Decodes the next argument, of type `ty` (with the call's type parameters), as an owned
/// object in boxed representation. After an error it returns `box(0)` and reads nothing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_read(call: *mut Call, ty: *const u8, ty_len: usize) -> Obj {
    let call = unsafe { &mut *call };
    if call.error.is_some() {
        return lean_box(0);
    }
    let ty = call.instantiate(&unsafe { generated_type(ty, ty_len) });
    let mut r = Reader::new(&call.input[call.pos..]);
    match wire::decode(call.table, &ty, &mut r, Handles::Borrow) {
        Ok(v) => {
            call.pos = call.input.len() - r.remaining();
            v
        }
        Err(e) => {
            call.fail(e);
            lean_box(0)
        }
    }
}

/// Whether every argument was decoded and the input fully consumed. The generated call only
/// runs the Lean function when it is.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_complete(call: *mut Call) -> bool {
    let call = unsafe { &mut *call };
    if call.error.is_none() && call.pos != call.input.len() {
        let extra = call.input.len() - call.pos;
        call.fail(WireError(format!("{extra} unexpected bytes after the arguments")));
    }
    call.error.is_none()
}

/// Encodes the result `v` (consumed) as a value of `ty`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_write(call: *mut Call, v: Obj, ty: *const u8, ty_len: usize) {
    let call = unsafe { &mut *call };
    let ty = call.instantiate(&unsafe { generated_type(ty, ty_len) });
    unsafe {
        wire::encode(call.table, &ty, v, &mut call.output);
        lean_dec(v);
    }
}

/// Encodes the `IO` result `r` (consumed): `OK` and the value of `ty`, or `ERROR` and the
/// `IO.Error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_write_io(call: *mut Call, r: Obj, ty: *const u8, ty_len: usize) {
    let call = unsafe { &mut *call };
    let ty = call.instantiate(&unsafe { generated_type(ty, ty_len) });
    unsafe {
        let v = lean_ctor_get(r, 0);
        if lean_io_result_is_ok(r) {
            call.output.push(result::OK);
            wire::encode(call.table, &ty, v, &mut call.output);
        } else {
            call.output.push(result::ERROR);
            wire::encode_io_error(v, &mut call.output);
        }
        lean_dec(r);
    }
}

/// Encodes the `EIO ε` result `r` (consumed): `OK` and the value of `value`, or `ERROR` and the
/// error of `error`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_write_eio(
    call: *mut Call,
    r: Obj,
    error: *const u8,
    error_len: usize,
    value: *const u8,
    value_len: usize,
) {
    let call = unsafe { &mut *call };
    let error = call.instantiate(&unsafe { generated_type(error, error_len) });
    let value = call.instantiate(&unsafe { generated_type(value, value_len) });
    unsafe {
        let v = lean_ctor_get(r, 0);
        if lean_io_result_is_ok(r) {
            call.output.push(result::OK);
            wire::encode(call.table, &value, v, &mut call.output);
        } else {
            call.output.push(result::ERROR);
            wire::encode(call.table, &error, v, &mut call.output);
        }
        lean_dec(r);
    }
}

/// Ends a call: stores the encoded result in `out` and returns `OK`, or stores the reason the
/// input was rejected and returns `MALFORMED`. Frees the call context.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_call_end(call: *mut Call, out: *mut Buffer) -> i32 {
    let call = unsafe { Box::from_raw(call) };
    let out = unsafe { &mut *out };
    match call.error {
        Some(msg) => {
            out.set(msg.into_bytes());
            MALFORMED
        }
        None => {
            out.set(call.output);
            OK
        }
    }
}

/// Calls the Lean closure of handle `handle`, of type `ty` (a function type expression, with the
/// input's type parameters), with the input: type arguments, then the arguments. Stores the
/// encoded result in `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_closure_call(
    types: *const TypeTable,
    handle: u64,
    ty: *const u8,
    ty_len: usize,
    input: *const u8,
    len: usize,
    out: *mut Buffer,
) -> i32 {
    let table = unsafe { table(types) };
    let out = unsafe { &mut *out };
    let input = if len == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(input, len) } };
    let ty = unsafe { std::slice::from_raw_parts(ty, ty_len) };
    let run = || -> Result<Vec<u8>, WireError> {
        let mut r = Reader::new(input);
        let args = type_args(table, &mut r)?;
        let ty = wire::parse_type(ty)?.substitute(&args)?;
        table.check_type(&ty, 0)?;
        let Type::Function { params, result } = ty else {
            return Err(WireError("a closure is called at a type that is not a function type".into()));
        };
        let mut values = Vec::new();
        for p in &params {
            match wire::decode(table, p, &mut r, Handles::Borrow) {
                Ok(v) => values.push(v),
                Err(e) => {
                    values.into_iter().for_each(|v| unsafe { lean_dec(v) });
                    return Err(e);
                }
            }
        }
        if let Err(e) = r.finish() {
            values.into_iter().for_each(|v| unsafe { lean_dec(v) });
            return Err(e);
        }
        let f = match wire::handle_get(handle) {
            Ok(f) => f,
            Err(e) => {
                values.into_iter().for_each(|v| unsafe { lean_dec(v) });
                return Err(e);
            }
        };
        let mut output = Vec::new();
        unsafe {
            let v = crate::apply::lean_apply_n(f, &values);
            wire::encode(table, &result, v, &mut output);
            lean_dec(v);
        }
        Ok(output)
    };
    match run() {
        Ok(bytes) => {
            out.set(bytes);
            OK
        }
        Err(e) => {
            out.set(e.0.into_bytes());
            MALFORMED
        }
    }
}

/// `lungo_hostcall`: encoding the arguments of a call to a host function and decoding its
/// result.
pub struct HostCall {
    table: &'static TypeTable,
    callback: u64,
    /// The Lean declaration the host implements, for diagnostics.
    declaration: String,
    input: Vec<u8>,
}

/// Starts a call to host function `callback` implementing the Lean extern `declaration`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_hostcall_begin(
    types: *const TypeTable,
    callback: u64,
    declaration: *const c_char,
) -> *mut HostCall {
    let table = unsafe { table(types) };
    let declaration = unsafe { CStr::from_ptr(declaration) }.to_string_lossy().into_owned();
    Box::into_raw(Box::new(HostCall { table, callback, declaration, input: Vec::new() }))
}

/// Encodes the argument `v` (borrowed) as a value of `ty`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_hostcall_write(call: *mut HostCall, v: Obj, ty: *const u8, ty_len: usize) {
    let call = unsafe { &mut *call };
    let ty = unsafe { generated_type(ty, ty_len) };
    unsafe { wire::encode(call.table, &ty, v, &mut call.input) };
}

fn host_failure(call: &HostCall, what: &str) -> ! {
    lean_internal_panic(&format!("the host implementation of Lean extern '{}' {what}", call.declaration))
}

/// Calls the host function and decodes its result, of type `ty`, as an owned object in boxed
/// representation. Frees the host call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_hostcall_finish(call: *mut HostCall, ty: *const u8, ty_len: usize) -> Obj {
    let call = unsafe { Box::from_raw(call) };
    let ty = unsafe { generated_type(ty, ty_len) };
    let bytes = wire::call_host(call.callback, &call.input)
        .unwrap_or_else(|msg| host_failure(&call, &format!("failed: {msg}")));
    let mut r = Reader::new(&bytes);
    match wire::decode(call.table, &ty, &mut r, Handles::Take).and_then(|v| r.finish().map(|_| v)) {
        Ok(v) => v,
        Err(e) => host_failure(&call, &format!("returned malformed data: {e}")),
    }
}

/// Calls the host function of an `IO` extern and returns the `IO` result: the value of `ty`, or
/// the `IO.Error` the host raised. A host function that fails is an `IO.userError` with its
/// message. Frees the host call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_hostcall_finish_io(call: *mut HostCall, ty: *const u8, ty_len: usize) -> Obj {
    let call = unsafe { Box::from_raw(call) };
    let ty = unsafe { generated_type(ty, ty_len) };
    let bytes = match wire::call_host(call.callback, &call.input) {
        Ok(bytes) => bytes,
        Err(msg) => unsafe { return lean_io_result_mk_error(crate::io::mk::user_error(lean_mk_string(&msg))) },
    };
    let mut r = Reader::new(&bytes);
    let decoded = r.u8().and_then(|tag| match tag {
        result::OK => wire::decode(call.table, &ty, &mut r, Handles::Take).map(|v| unsafe { lean_io_result_mk_ok(v) }),
        result::ERROR => wire::decode_io_error(&mut r).map(|e| unsafe { lean_io_result_mk_error(e) }),
        t => Err(WireError(format!("invalid IO result tag {t}"))),
    });
    match decoded.and_then(|v| r.finish().map(|_| v)) {
        Ok(v) => v,
        Err(e) => host_failure(&call, &format!("returned malformed data: {e}")),
    }
}

/// Calls the host function of an `EIO ε` extern and returns the result: the value of `value`, or
/// the error of `error` the host returned. Frees the host call.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_hostcall_finish_eio(
    call: *mut HostCall,
    error: *const u8,
    error_len: usize,
    value: *const u8,
    value_len: usize,
) -> Obj {
    let call = unsafe { Box::from_raw(call) };
    let error = unsafe { generated_type(error, error_len) };
    let value = unsafe { generated_type(value, value_len) };
    let bytes = wire::call_host(call.callback, &call.input)
        .unwrap_or_else(|msg| host_failure(&call, &format!("failed: {msg}")));
    let mut r = Reader::new(&bytes);
    let decoded = r.u8().and_then(|tag| match tag {
        result::OK => wire::decode(call.table, &value, &mut r, Handles::Take).map(|v| unsafe { lean_io_result_mk_ok(v) }),
        result::ERROR => wire::decode(call.table, &error, &mut r, Handles::Take).map(|e| unsafe { lean_io_result_mk_error(e) }),
        t => Err(WireError(format!("invalid EIO result tag {t}"))),
    });
    match decoded.and_then(|v| r.finish().map(|_| v)) {
        Ok(v) => v,
        Err(e) => host_failure(&call, &format!("returned malformed data: {e}")),
    }
}

/// Installs the host's entry points, once per process.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_set_host(
    dispatch: unsafe extern "C" fn(callback: u64, input: *const u8, len: usize, out: *mut Buffer) -> i32,
    retain: unsafe extern "C" fn(callback: u64),
    release: unsafe extern "C" fn(callback: u64),
) {
    wire::set_host(Host { dispatch, retain, release });
}

/// Releases handle `handle`. Releasing a handle that is not live is an error of the binding.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_handle_release(handle: u64) {
    if let Err(e) = wire::handle_release(handle) {
        lean_internal_panic(&format!("lungo_handle_release: {e}"));
    }
}

/// A new handle to the object of handle `handle`: a host that returns a value it received passes
/// a clone, since a result transfers its handles. Cloning a handle that is not live is an error
/// of the binding.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_handle_clone(handle: u64) -> u64 {
    wire::handle_clone(handle).unwrap_or_else(|e| lean_internal_panic(&format!("lungo_handle_clone: {e}")))
}

/// Allocates `len` bytes in `out` (which must be empty) for a host to fill, and returns them.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_buffer_alloc(out: *mut Buffer, len: usize) -> *mut u8 {
    let out = unsafe { &mut *out };
    out.set(vec![0; len]);
    out.data
}

/// Frees the bytes of `buffer`, leaving it empty.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_buffer_free(buffer: *mut Buffer) {
    drop(unsafe { &mut *buffer }.take());
}
