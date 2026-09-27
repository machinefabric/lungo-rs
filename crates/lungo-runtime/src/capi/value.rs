//! The C value API: Lean values as trees of `lungo_value`, typed by `lungo_type`, and calls of
//! a program's functions with them.
//!
//! This is the C binding of the wire format. A generated C package wraps it with a typed API
//! (a function per export, named constructors and accessors per type), but every value is a
//! `lungo_value`, so polymorphic functions, recursive types and closures work uniformly.
//!
//! Ownership:
//! - A `lungo_value *` returned by the API is owned by the caller, who frees it with
//!   `lungo_value_free`. Constructors of composite values take ownership of their parts.
//!   Accessors return borrowed pointers into their argument.
//! - A value owns the handles in it (opaque values, Lean closures) and the references to the
//!   host functions in it: freeing it releases them, cloning it duplicates them.
//! - `lungo_type *` and `lungo_error *` are likewise owned by the caller.
//!
//! Misuse of the API (a null pointer, an accessor of another kind of value, an index out of
//! range) terminates the process with a message; invalid *data* (a string that is not UTF-8, a
//! value that does not match the parameter type) is reported to the caller.
//!
//! Host functions written in C are values too (`lungo_value_function`), and implement host
//! externs: the C host, installed the first time one is created, dispatches Lean's calls to
//! them. A process has one host, so the C value API cannot be combined with another language's
//! support library in the same process.

use super::boundary::{MALFORMED, OK};
use crate::object::lean_internal_panic;
use crate::wire::value::{self as wv, FunctionRef, Value};
use crate::wire::{self, Buffer, Host, Reader, Returns, Signature, Type, TypeTable, WireError, result, tag};
use num_bigint::{BigInt, BigUint};
use num_traits::ToPrimitive;
use std::collections::HashMap;
use std::ffi::{CStr, CString, c_char, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once, OnceLock};

/// Status of a call through the value API: the function failed (an `IO` or `EIO` error, or a
/// host function's failure); the error describes why.
pub const FAILED: i32 = 2;

/// Kinds of errors.
pub mod error_kind {
    /// An `IO.Error` of Lean, or one a host function raises.
    pub const IO: u32 = 0;
    /// An error value of an `EIO ε` function.
    pub const VALUE: u32 = 1;
    /// Arguments that do not match the function's parameters.
    pub const MALFORMED: u32 = 2;
}

/// A type expression (`lungo_type`), with the type table of the program its inductive types
/// belong to.
#[derive(Clone)]
pub struct TypeExpr {
    table: Option<&'static TypeTable>,
    ty: Type,
}

/// An error (`lungo_error`).
pub struct Error {
    data: ErrorData,
    message: CString,
}

enum ErrorData {
    /// An `IO.Error`: a handle to it (0 for a host's `IO.userError`).
    Io(u64),
    Value(Value),
    Malformed,
}

/// A C host function: `ctx` and the arguments (borrowed) in, a result or an error out.
pub type CFunction = unsafe extern "C" fn(
    ctx: *mut c_void,
    args: *const *const Value,
    n: usize,
    result: *mut *mut Value,
    error: *mut *mut Error,
) -> i32;

/// Frees a host function's context.
pub type CDrop = unsafe extern "C" fn(ctx: *mut c_void);

fn misuse(what: &str, msg: &str) -> ! {
    lean_internal_panic(&format!("{what}: {msg}"))
}

unsafe fn borrow<'a, T>(p: *const T, what: &str) -> &'a T {
    if p.is_null() {
        misuse(what, "a null pointer");
    }
    unsafe { &*p }
}

unsafe fn take<T>(p: *mut T, what: &str) -> T {
    if p.is_null() {
        misuse(what, "a null pointer");
    }
    *unsafe { Box::from_raw(p) }
}

unsafe fn out_ptr<'a, T>(p: *mut T, what: &str) -> &'a mut T {
    if p.is_null() {
        misuse(what, "a null pointer");
    }
    unsafe { &mut *p }
}

fn give<T>(v: T) -> *mut T {
    Box::into_raw(Box::new(v))
}

fn empty_table() -> &'static TypeTable {
    static T: OnceLock<TypeTable> = OnceLock::new();
    T.get_or_init(|| TypeTable { types: Vec::new() })
}

/// The table of a type built from parts of tables `a` and `b`.
fn join(a: Option<&'static TypeTable>, b: Option<&'static TypeTable>, what: &str) -> Option<&'static TypeTable> {
    match (a, b) {
        (Some(x), Some(y)) if !std::ptr::eq(x, y) => misuse(what, "types of different programs"),
        (Some(x), _) | (_, Some(x)) => Some(x),
        (None, None) => None,
    }
}

fn message(s: &str) -> CString {
    CString::new(s.replace('\0', "\u{fffd}")).expect("NUL bytes were replaced")
}

fn malformed(e: impl std::fmt::Display) -> Error {
    Error { data: ErrorData::Malformed, message: message(&e.to_string()) }
}

// ---------------------------------------------------------------------------------------------
// Handle ownership
// ---------------------------------------------------------------------------------------------

/// Releases the handles and host references `v` owns.
fn release(v: &Value) {
    match v {
        Value::Opaque(h) | Value::Function(FunctionRef::Lean(h)) => {
            if let Err(e) = wire::handle_release(*h) {
                lean_internal_panic(&format!("lungo_value_free: {e} (was the value freed twice?)"));
            }
        }
        Value::Function(FunctionRef::Host(id)) => host_release(*id),
        Value::Option(Some(x)) => release(x),
        Value::List(xs) | Value::Array(xs) => xs.iter().for_each(release),
        Value::Ctor { fields, .. } => fields.iter().for_each(release),
        Value::Prod(a, b) => {
            release(a);
            release(b);
        }
        Value::Except(Ok(x)) | Value::Except(Err(x)) => release(x),
        _ => {}
    }
}

/// Makes `v`, a copy, own its own handles and host references.
fn retain(v: &mut Value) {
    match v {
        Value::Opaque(h) | Value::Function(FunctionRef::Lean(h)) => {
            *h = wire::handle_clone(*h)
                .unwrap_or_else(|e| lean_internal_panic(&format!("lungo_value_clone: {e} (was the value freed?)")));
        }
        Value::Function(FunctionRef::Host(id)) => host_retain(*id),
        Value::Option(Some(x)) => retain(x),
        Value::List(xs) | Value::Array(xs) => xs.iter_mut().for_each(retain),
        Value::Ctor { fields, .. } => fields.iter_mut().for_each(retain),
        Value::Prod(a, b) => {
            retain(a);
            retain(b);
        }
        Value::Except(Ok(x)) | Value::Except(Err(x)) => retain(x),
        _ => {}
    }
}

fn free_value(v: Value) {
    release(&v);
}

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

/// A type without parameters: `tag` is one of `LUNGO_NAT` … `LUNGO_FLOAT_ARRAY` or `LUNGO_OPAQUE`.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_type_simple(t: u8) -> *mut TypeExpr {
    let ty = match t {
        tag::NAT => Type::Nat,
        tag::INT => Type::Int,
        tag::BOOL => Type::Bool,
        tag::UINT8 => Type::UInt8,
        tag::UINT16 => Type::UInt16,
        tag::UINT32 => Type::UInt32,
        tag::UINT64 => Type::UInt64,
        tag::USIZE => Type::USize,
        tag::INT8 => Type::Int8,
        tag::INT16 => Type::Int16,
        tag::INT32 => Type::Int32,
        tag::INT64 => Type::Int64,
        tag::ISIZE => Type::ISize,
        tag::FLOAT => Type::Float,
        tag::FLOAT32 => Type::Float32,
        tag::CHAR => Type::Char,
        tag::STRING => Type::String,
        tag::UNIT => Type::Unit,
        tag::BYTE_ARRAY => Type::ByteArray,
        tag::FLOAT_ARRAY => Type::FloatArray,
        tag::OPAQUE => Type::Opaque,
        t => misuse("lungo_type_simple", &format!("{t} is not the kind of a type without parameters")),
    };
    give(TypeExpr { table: None, ty })
}

unsafe fn unary(t: *const TypeExpr, what: &str, f: fn(Box<Type>) -> Type) -> *mut TypeExpr {
    let t = unsafe { borrow(t, what) };
    give(TypeExpr { table: t.table, ty: f(Box::new(t.ty.clone())) })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_option(t: *const TypeExpr) -> *mut TypeExpr {
    unsafe { unary(t, "lungo_type_option", Type::Option) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_list(t: *const TypeExpr) -> *mut TypeExpr {
    unsafe { unary(t, "lungo_type_list", Type::List) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_array(t: *const TypeExpr) -> *mut TypeExpr {
    unsafe { unary(t, "lungo_type_array", Type::Array) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_prod(a: *const TypeExpr, b: *const TypeExpr) -> *mut TypeExpr {
    let what = "lungo_type_prod";
    let (a, b) = unsafe { (borrow(a, what), borrow(b, what)) };
    give(TypeExpr {
        table: join(a.table, b.table, what),
        ty: Type::Prod(Box::new(a.ty.clone()), Box::new(b.ty.clone())),
    })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_except(error: *const TypeExpr, value: *const TypeExpr) -> *mut TypeExpr {
    let what = "lungo_type_except";
    let (e, v) = unsafe { (borrow(error, what), borrow(value, what)) };
    give(TypeExpr {
        table: join(e.table, v.table, what),
        ty: Type::Except { error: Box::new(e.ty.clone()), value: Box::new(v.ty.clone()) },
    })
}

/// The type of functions from `n` (1 to 15) parameters to `result`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_function(
    params: *const *const TypeExpr,
    n: usize,
    result: *const TypeExpr,
) -> *mut TypeExpr {
    let what = "lungo_type_function";
    if n == 0 || n > wire::MAX_FUNCTION_PARAMS {
        misuse(what, &format!("a function of {n} parameters (1 to {} are supported)", wire::MAX_FUNCTION_PARAMS));
    }
    let params = unsafe { std::slice::from_raw_parts(borrow(params, what), n) };
    let result = unsafe { borrow(result, what) };
    let mut table = result.table;
    let mut ps = Vec::new();
    for p in params {
        let p = unsafe { borrow(*p, what) };
        table = join(table, p.table, what);
        ps.push(p.ty.clone());
    }
    give(TypeExpr { table, ty: Type::Function { params: ps, result: Box::new(result.ty.clone()) } })
}

/// The inductive type at `index` of the program's table `types`, applied to `n` arguments.
/// Generated packages call this for their named types.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_inductive(
    types: *const TypeTable,
    index: u32,
    args: *const *const TypeExpr,
    n: usize,
) -> *mut TypeExpr {
    let what = "lungo_type_inductive";
    let types: &'static TypeTable = unsafe { borrow(types, what) };
    let args = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(borrow(args, what), n) } };
    let mut table = Some(types);
    let mut tys = Vec::new();
    for a in args {
        let a = unsafe { borrow(*a, what) };
        table = join(table, a.table, what);
        tys.push(a.ty.clone());
    }
    let ty = Type::Inductive { index, args: tys };
    if let Err(e) = types.check_type(&ty, 0) {
        misuse(what, &e.0);
    }
    give(TypeExpr { table, ty })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_clone(t: *const TypeExpr) -> *mut TypeExpr {
    give(unsafe { borrow(t, "lungo_type_clone") }.clone())
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_type_free(t: *mut TypeExpr) {
    if !t.is_null() {
        drop(unsafe { Box::from_raw(t) });
    }
}

// ---------------------------------------------------------------------------------------------
// Values
// ---------------------------------------------------------------------------------------------

/// The kind of `v`: the tag of its type (`LUNGO_NAT` …), `LUNGO_INDUCTIVE` for a constructor.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_kind(v: *const Value) -> u8 {
    kind(unsafe { borrow(v, "lungo_value_kind") })
}

fn kind(v: &Value) -> u8 {
    match v {
        Value::Nat(_) => tag::NAT,
        Value::Int(_) => tag::INT,
        Value::Bool(_) => tag::BOOL,
        Value::UInt8(_) => tag::UINT8,
        Value::UInt16(_) => tag::UINT16,
        Value::UInt32(_) => tag::UINT32,
        Value::UInt64(_) => tag::UINT64,
        Value::USize(_) => tag::USIZE,
        Value::Int8(_) => tag::INT8,
        Value::Int16(_) => tag::INT16,
        Value::Int32(_) => tag::INT32,
        Value::Int64(_) => tag::INT64,
        Value::ISize(_) => tag::ISIZE,
        Value::Float(_) => tag::FLOAT,
        Value::Float32(_) => tag::FLOAT32,
        Value::Char(_) => tag::CHAR,
        Value::String(_) => tag::STRING,
        Value::Unit => tag::UNIT,
        Value::ByteArray(_) => tag::BYTE_ARRAY,
        Value::FloatArray(_) => tag::FLOAT_ARRAY,
        Value::Option(_) => tag::OPTION,
        Value::List(_) => tag::LIST,
        Value::Array(_) => tag::ARRAY,
        Value::Prod(..) => tag::PROD,
        Value::Except(_) => tag::EXCEPT,
        Value::Function(_) => tag::FUNCTION,
        Value::Opaque(_) => tag::OPAQUE,
        Value::Ctor { .. } => tag::INDUCTIVE,
    }
}

fn kind_name(v: &Value) -> &'static str {
    match v {
        Value::Nat(_) => "a Nat",
        Value::Int(_) => "an Int",
        Value::Bool(_) => "a Bool",
        Value::UInt8(_) => "a UInt8",
        Value::UInt16(_) => "a UInt16",
        Value::UInt32(_) => "a UInt32",
        Value::UInt64(_) => "a UInt64",
        Value::USize(_) => "a USize",
        Value::Int8(_) => "an Int8",
        Value::Int16(_) => "an Int16",
        Value::Int32(_) => "an Int32",
        Value::Int64(_) => "an Int64",
        Value::ISize(_) => "an ISize",
        Value::Float(_) => "a Float",
        Value::Float32(_) => "a Float32",
        Value::Char(_) => "a Char",
        Value::String(_) => "a String",
        Value::Unit => "Unit",
        Value::ByteArray(_) => "a ByteArray",
        Value::FloatArray(_) => "a FloatArray",
        Value::Option(_) => "an Option",
        Value::List(_) => "a List",
        Value::Array(_) => "an Array",
        Value::Prod(..) => "a pair",
        Value::Except(_) => "an Except",
        Value::Function(_) => "a function",
        Value::Opaque(_) => "an opaque value",
        Value::Ctor { .. } => "a constructor",
    }
}

fn wrong_kind(what: &str, v: &Value) -> ! {
    misuse(what, &format!("the value is {}", kind_name(v)))
}

macro_rules! scalar_values {
    ($($new:ident $get:ident $variant:ident $t:ty;)*) => {
        $(
            #[unsafe(no_mangle)]
            pub extern "C" fn $new(x: $t) -> *mut Value {
                give(Value::$variant(x))
            }

            #[unsafe(no_mangle)]
            pub unsafe extern "C" fn $get(v: *const Value) -> $t {
                match unsafe { borrow(v, stringify!($get)) } {
                    Value::$variant(x) => *x,
                    other => wrong_kind(stringify!($get), other),
                }
            }
        )*
    };
}

scalar_values! {
    lungo_value_bool lungo_value_get_bool Bool bool;
    lungo_value_uint8 lungo_value_get_uint8 UInt8 u8;
    lungo_value_uint16 lungo_value_get_uint16 UInt16 u16;
    lungo_value_uint32 lungo_value_get_uint32 UInt32 u32;
    lungo_value_uint64 lungo_value_get_uint64 UInt64 u64;
    lungo_value_usize lungo_value_get_usize USize u64;
    lungo_value_int8 lungo_value_get_int8 Int8 i8;
    lungo_value_int16 lungo_value_get_int16 Int16 i16;
    lungo_value_int32 lungo_value_get_int32 Int32 i32;
    lungo_value_int64 lungo_value_get_int64 Int64 i64;
    lungo_value_isize lungo_value_get_isize ISize i64;
    lungo_value_float lungo_value_get_float Float f64;
    lungo_value_float32 lungo_value_get_float32 Float32 f32;
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_value_nat(x: u64) -> *mut Value {
    give(Value::Nat(BigUint::from(x)))
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_value_int(x: i64) -> *mut Value {
    give(Value::Int(BigInt::from(x)))
}

unsafe fn c_text<'a>(s: *const c_char, what: &str) -> &'a [u8] {
    unsafe { CStr::from_ptr(borrow(s, what)) }.to_bytes()
}

/// A `Nat` from its decimal digits, or `NULL` if `digits` is not a decimal natural number.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_nat_parse(digits: *const c_char) -> *mut Value {
    let s = unsafe { c_text(digits, "lungo_value_nat_parse") };
    if s.is_empty() || !s.iter().all(u8::is_ascii_digit) {
        return std::ptr::null_mut();
    }
    match BigUint::parse_bytes(s, 10) {
        Some(n) => give(Value::Nat(n)),
        None => std::ptr::null_mut(),
    }
}

/// An `Int` from its decimal digits with an optional leading `-`, or `NULL` if `digits` is not a
/// decimal integer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_int_parse(digits: *const c_char) -> *mut Value {
    let s = unsafe { c_text(digits, "lungo_value_int_parse") };
    let body = s.strip_prefix(b"-").unwrap_or(s);
    if body.is_empty() || !body.iter().all(u8::is_ascii_digit) {
        return std::ptr::null_mut();
    }
    match BigInt::parse_bytes(s, 10) {
        Some(n) => give(Value::Int(n)),
        None => std::ptr::null_mut(),
    }
}

/// Stores the `Nat` `v` in `out` if it fits 64 bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_nat(v: *const Value, out: *mut u64) -> bool {
    let what = "lungo_value_get_nat";
    match unsafe { borrow(v, what) } {
        Value::Nat(n) => match n.to_u64() {
            Some(x) => {
                unsafe { *out_ptr(out, what) = x };
                true
            }
            None => false,
        },
        other => wrong_kind(what, other),
    }
}

/// Stores the `Int` `v` in `out` if it fits 64 bits.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_int(v: *const Value, out: *mut i64) -> bool {
    let what = "lungo_value_get_int";
    match unsafe { borrow(v, what) } {
        Value::Int(n) => match n.to_i64() {
            Some(x) => {
                unsafe { *out_ptr(out, what) = x };
                true
            }
            None => false,
        },
        other => wrong_kind(what, other),
    }
}

/// The decimal digits of the `Nat` or `Int` `v`, freed with `lungo_string_free`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_number_string(v: *const Value) -> *mut c_char {
    let what = "lungo_value_number_string";
    let s = match unsafe { borrow(v, what) } {
        Value::Nat(n) => n.to_string(),
        Value::Int(n) => n.to_string(),
        other => wrong_kind(what, other),
    };
    message(&s).into_raw()
}

/// A `Char`, or `NULL` if `c` is not a Unicode scalar value.
#[unsafe(no_mangle)]
pub extern "C" fn lungo_value_char(c: u32) -> *mut Value {
    match char::from_u32(c) {
        Some(c) => give(Value::Char(c)),
        None => std::ptr::null_mut(),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_char(v: *const Value) -> u32 {
    match unsafe { borrow(v, "lungo_value_get_char") } {
        Value::Char(c) => *c as u32,
        other => wrong_kind("lungo_value_get_char", other),
    }
}

unsafe fn bytes<'a>(data: *const u8, len: usize, what: &str) -> &'a [u8] {
    if len == 0 { &[] } else { unsafe { std::slice::from_raw_parts(borrow(data, what), len) } }
}

/// A `String` of the `len` bytes at `utf8`, or `NULL` if they are not UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_string(utf8: *const c_char, len: usize) -> *mut Value {
    let b = unsafe { bytes(utf8 as *const u8, len, "lungo_value_string") };
    match std::str::from_utf8(b) {
        Ok(s) => give(Value::String(s.to_owned())),
        Err(_) => std::ptr::null_mut(),
    }
}

/// A `String` of the NUL-terminated `utf8`, or `NULL` if it is not UTF-8.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_cstring(utf8: *const c_char) -> *mut Value {
    match std::str::from_utf8(unsafe { c_text(utf8, "lungo_value_cstring") }) {
        Ok(s) => give(Value::String(s.to_owned())),
        Err(_) => std::ptr::null_mut(),
    }
}

/// The UTF-8 bytes of the `String` `v` and, in `len`, their number. The bytes are not
/// NUL-terminated (a Lean string may contain NUL); see `lungo_value_string_dup`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_string(v: *const Value, len: *mut usize) -> *const c_char {
    let what = "lungo_value_get_string";
    match unsafe { borrow(v, what) } {
        Value::String(s) => {
            unsafe { *out_ptr(len, what) = s.len() };
            s.as_ptr() as *const c_char
        }
        other => wrong_kind(what, other),
    }
}

/// A NUL-terminated copy of the `String` `v`, freed with `lungo_string_free`, or `NULL` if the
/// string contains NUL.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_string_dup(v: *const Value) -> *mut c_char {
    let what = "lungo_value_string_dup";
    match unsafe { borrow(v, what) } {
        Value::String(s) => CString::new(s.as_str()).map(CString::into_raw).unwrap_or(std::ptr::null_mut()),
        other => wrong_kind(what, other),
    }
}

/// Frees a string the API returned.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_value_unit() -> *mut Value {
    give(Value::Unit)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_byte_array(data: *const u8, len: usize) -> *mut Value {
    give(Value::ByteArray(unsafe { bytes(data, len, "lungo_value_byte_array") }.to_vec()))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_bytes(v: *const Value, len: *mut usize) -> *const u8 {
    let what = "lungo_value_get_bytes";
    match unsafe { borrow(v, what) } {
        Value::ByteArray(b) => {
            unsafe { *out_ptr(len, what) = b.len() };
            b.as_ptr()
        }
        other => wrong_kind(what, other),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_float_array(data: *const f64, len: usize) -> *mut Value {
    let xs = if len == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(borrow(data, "lungo_value_float_array"), len) }.to_vec()
    };
    give(Value::FloatArray(xs))
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_floats(v: *const Value, len: *mut usize) -> *const f64 {
    let what = "lungo_value_get_floats";
    match unsafe { borrow(v, what) } {
        Value::FloatArray(xs) => {
            unsafe { *out_ptr(len, what) = xs.len() };
            xs.as_ptr()
        }
        other => wrong_kind(what, other),
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn lungo_value_none() -> *mut Value {
    give(Value::Option(None))
}

/// `some x`, taking ownership of `x`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_some(x: *mut Value) -> *mut Value {
    give(Value::Option(Some(Box::new(unsafe { take(x, "lungo_value_some") }))))
}

/// The value of the `Option` `v`, or `NULL` for `none`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_option(v: *const Value) -> *const Value {
    let what = "lungo_value_get_option";
    match unsafe { borrow(v, what) } {
        Value::Option(Some(x)) => &**x,
        Value::Option(None) => std::ptr::null(),
        other => wrong_kind(what, other),
    }
}

unsafe fn take_all(items: *mut *mut Value, n: usize, what: &str) -> Vec<Value> {
    if n == 0 {
        return Vec::new();
    }
    let items = unsafe { std::slice::from_raw_parts(borrow(items, what), n) };
    items.iter().map(|x| unsafe { take(*x, what) }).collect()
}

/// A `List` of the `n` values `items`, taking ownership of them (not of the array).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_list(items: *mut *mut Value, n: usize) -> *mut Value {
    give(Value::List(unsafe { take_all(items, n, "lungo_value_list") }))
}

/// An `Array` of the `n` values `items`, taking ownership of them (not of the array).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_array(items: *mut *mut Value, n: usize) -> *mut Value {
    give(Value::Array(unsafe { take_all(items, n, "lungo_value_array") }))
}

/// The number of items of the `List` or `Array` `v`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_count(v: *const Value) -> usize {
    match unsafe { borrow(v, "lungo_value_count") } {
        Value::List(xs) | Value::Array(xs) => xs.len(),
        other => wrong_kind("lungo_value_count", other),
    }
}

/// Item `i` of the `List` or `Array` `v`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_item(v: *const Value, i: usize) -> *const Value {
    let what = "lungo_value_item";
    match unsafe { borrow(v, what) } {
        Value::List(xs) | Value::Array(xs) => {
            xs.get(i).unwrap_or_else(|| misuse(what, &format!("index {i} of {} items", xs.len())))
        }
        other => wrong_kind(what, other),
    }
}

/// The pair `(a, b)`, taking ownership of both.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_prod(a: *mut Value, b: *mut Value) -> *mut Value {
    let what = "lungo_value_prod";
    unsafe { give(Value::Prod(Box::new(take(a, what)), Box::new(take(b, what)))) }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_first(v: *const Value) -> *const Value {
    match unsafe { borrow(v, "lungo_value_first") } {
        Value::Prod(a, _) => &**a,
        other => wrong_kind("lungo_value_first", other),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_second(v: *const Value) -> *const Value {
    match unsafe { borrow(v, "lungo_value_second") } {
        Value::Prod(_, b) => &**b,
        other => wrong_kind("lungo_value_second", other),
    }
}

/// `Except.ok x`, taking ownership of `x`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_ok(x: *mut Value) -> *mut Value {
    give(Value::Except(Ok(Box::new(unsafe { take(x, "lungo_value_ok") }))))
}

/// `Except.error e`, taking ownership of `e`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_error(e: *mut Value) -> *mut Value {
    give(Value::Except(Err(Box::new(unsafe { take(e, "lungo_value_error") }))))
}

/// Whether the `Except` `v` is `ok`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_is_ok(v: *const Value) -> bool {
    match unsafe { borrow(v, "lungo_value_is_ok") } {
        Value::Except(r) => r.is_ok(),
        other => wrong_kind("lungo_value_is_ok", other),
    }
}

/// The value or error of the `Except` `v`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_get_except(v: *const Value) -> *const Value {
    match unsafe { borrow(v, "lungo_value_get_except") } {
        Value::Except(Ok(x)) | Value::Except(Err(x)) => &**x,
        other => wrong_kind("lungo_value_get_except", other),
    }
}

/// Constructor `index` (in its type's declaration order) applied to the `n` values `fields`,
/// taking ownership of them (not of the array). Generated packages call this for their named
/// constructors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_ctor(index: u32, fields: *mut *mut Value, n: usize) -> *mut Value {
    give(Value::Ctor { index, fields: unsafe { take_all(fields, n, "lungo_value_ctor") } })
}

/// The constructor index of the value `v` of an inductive type.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_ctor_index(v: *const Value) -> u32 {
    match unsafe { borrow(v, "lungo_value_ctor_index") } {
        Value::Ctor { index, .. } => *index,
        other => wrong_kind("lungo_value_ctor_index", other),
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_field_count(v: *const Value) -> usize {
    match unsafe { borrow(v, "lungo_value_field_count") } {
        Value::Ctor { fields, .. } => fields.len(),
        other => wrong_kind("lungo_value_field_count", other),
    }
}

/// Field `i` of the constructor value `v`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_field(v: *const Value, i: usize) -> *const Value {
    let what = "lungo_value_field";
    match unsafe { borrow(v, what) } {
        Value::Ctor { fields, .. } => {
            fields.get(i).unwrap_or_else(|| misuse(what, &format!("field {i} of {}", fields.len())))
        }
        other => wrong_kind(what, other),
    }
}

/// Field `i` of the value `v` of constructor `ctor`: a typed accessor of a generated package.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_ctor_field(v: *const Value, ctor: u32, i: usize) -> *const Value {
    let what = "lungo_value_ctor_field";
    match unsafe { borrow(v, what) } {
        Value::Ctor { index, fields } if *index == ctor => {
            fields.get(i).unwrap_or_else(|| misuse(what, &format!("field {i} of {}", fields.len())))
        }
        Value::Ctor { index, .. } => misuse(what, &format!("the value is constructor {index}, not constructor {ctor}")),
        other => wrong_kind(what, other),
    }
}

/// A copy of `v`, with its own handles and host references.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_clone(v: *const Value) -> *mut Value {
    let mut c = unsafe { borrow(v, "lungo_value_clone") }.clone();
    retain(&mut c);
    give(c)
}

/// Whether `a` and `b` are equal: structurally, with opaque values and functions equal when
/// they are the same handle or host function.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_equal(a: *const Value, b: *const Value) -> bool {
    unsafe { borrow(a, "lungo_value_equal") == borrow(b, "lungo_value_equal") }
}

/// Frees `v` and releases what it owns. `NULL` is ignored.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_free(v: *mut Value) {
    if !v.is_null() {
        free_value(*unsafe { Box::from_raw(v) });
    }
}

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_kind(e: *const Error) -> u32 {
    match unsafe { borrow(e, "lungo_error_kind") }.data {
        ErrorData::Io(_) => error_kind::IO,
        ErrorData::Value(_) => error_kind::VALUE,
        ErrorData::Malformed => error_kind::MALFORMED,
    }
}

/// The error's message, for display: the `IO.Error` as Lean renders it, the reason arguments
/// were rejected, or, for an error value, the value if it is a string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_message(e: *const Error) -> *const c_char {
    unsafe { borrow(e, "lungo_error_message") }.message.as_ptr()
}

/// The error value of an `EIO` error, or `NULL` for other errors.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_value(e: *const Error) -> *const Value {
    match &unsafe { borrow(e, "lungo_error_value") }.data {
        ErrorData::Value(v) => v,
        _ => std::ptr::null(),
    }
}

/// An `IO.userError` with the NUL-terminated UTF-8 `message`, for a host function to raise.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_io(msg: *const c_char) -> *mut Error {
    let what = "lungo_error_io";
    let text = std::str::from_utf8(unsafe { c_text(msg, what) }).unwrap_or_else(|_| misuse(what, "the message is not UTF-8"));
    give(Error { data: ErrorData::Io(0), message: message(text) })
}

/// An error value for a host function of an `EIO ε` extern to return, taking ownership of `v`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_from_value(v: *mut Value) -> *mut Error {
    give(value_error(unsafe { take(v, "lungo_error_from_value") }))
}

fn value_error(v: Value) -> Error {
    let text = match &v {
        Value::String(s) => s.clone(),
        _ => "a Lean error value (see lungo_error_value)".to_owned(),
    };
    Error { data: ErrorData::Value(v), message: message(&text) }
}

/// A copy of `e`, with its own handles: a host function re-raises an error of Lean it received
/// by returning a copy.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_clone(e: *const Error) -> *mut Error {
    let e = unsafe { borrow(e, "lungo_error_clone") };
    let data = match &e.data {
        ErrorData::Io(0) => ErrorData::Io(0),
        ErrorData::Io(h) => ErrorData::Io(
            wire::handle_clone(*h).unwrap_or_else(|err| misuse("lungo_error_clone", &format!("{err} (was the error freed?)"))),
        ),
        ErrorData::Value(v) => {
            let mut c = v.clone();
            retain(&mut c);
            ErrorData::Value(c)
        }
        ErrorData::Malformed => ErrorData::Malformed,
    };
    give(Error { data, message: e.message.clone() })
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_error_free(e: *mut Error) {
    if e.is_null() {
        return;
    }
    let e = *unsafe { Box::from_raw(e) };
    match e.data {
        ErrorData::Io(0) | ErrorData::Malformed => {}
        ErrorData::Io(h) => {
            if let Err(err) = wire::handle_release(h) {
                lean_internal_panic(&format!("lungo_error_free: {err} (was the error freed twice?)"));
            }
        }
        ErrorData::Value(v) => free_value(v),
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding and decoding with signatures
// ---------------------------------------------------------------------------------------------

/// Encodes `args` as the parameters `params`.
fn encode_args(table: &TypeTable, params: &[Type], args: &[&Value], out: &mut Vec<u8>) -> Result<(), WireError> {
    if args.len() != params.len() {
        return Err(WireError(format!("{} arguments for {} parameters", args.len(), params.len())));
    }
    for (i, (p, a)) in params.iter().zip(args).enumerate() {
        wv::encode(table, p, a, out).map_err(|e| WireError(format!("argument {}: {e}", i + 1)))?;
    }
    Ok(())
}

/// Decodes a complete result of `returns` produced by the runtime.
fn decode_result(table: &TypeTable, returns: &Returns, bytes: &[u8]) -> Result<Result<Value, Error>, WireError> {
    let mut r = Reader::new(bytes);
    let out = match returns {
        Returns::Value(t) => Ok(wv::decode(table, t, &mut r)?),
        Returns::Io(t) => match r.u8()? {
            result::OK => Ok(wv::decode(table, t, &mut r)?),
            result::ERROR => {
                let handle = r.u64()?;
                let text = r.string()?;
                Err(Error { data: ErrorData::Io(handle), message: message(&text) })
            }
            k => return Err(WireError(format!("invalid IO result tag {k}"))),
        },
        Returns::Eio { error, value } => match r.u8()? {
            result::OK => Ok(wv::decode(table, value, &mut r)?),
            result::ERROR => Err(value_error(wv::decode(table, error, &mut r)?)),
            k => return Err(WireError(format!("invalid EIO result tag {k}"))),
        },
    };
    r.finish()?;
    Ok(out)
}

/// Stores the outcome of a call in `result` or `error`, returning its status.
unsafe fn complete(outcome: Result<Value, Error>, status: i32, result: *mut *mut Value, error: *mut *mut Error) -> i32 {
    unsafe {
        *result = std::ptr::null_mut();
        *error = std::ptr::null_mut();
        match outcome {
            Ok(v) => {
                *result = give(v);
                OK
            }
            Err(e) => {
                *error = give(e);
                status
            }
        }
    }
}

/// The runtime's result of a call it made: decoding it cannot fail unless the runtime is
/// defective.
fn runtime_result(table: &TypeTable, returns: &Returns, bytes: &[u8], what: &str) -> Result<Value, Error> {
    decode_result(table, returns, bytes)
        .unwrap_or_else(|e| lean_internal_panic(&format!("{what}: the runtime produced a malformed result: {e}")))
}

/// A generated call entry point: `<prefix>call_<function>`.
pub type Entry = unsafe extern "C" fn(input: *const u8, len: usize, out: *mut Buffer) -> i32;

/// Calls a program's function through its entry point `entry`, of the generated signature
/// `sig`, instantiated with `n_types` type arguments, with `n_args` arguments (borrowed). On
/// `LUNGO_OK` stores the result in `result`; otherwise stores the error in `error`: the Lean
/// function's error (`LUNGO_FAILED`), or why the arguments were rejected (`LUNGO_MALFORMED`).
#[unsafe(no_mangle)]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn lungo_invoke(
    types: *const TypeTable,
    entry: Entry,
    sig: *const u8,
    sig_len: usize,
    type_args: *const *const TypeExpr,
    n_types: usize,
    args: *const *const Value,
    n_args: usize,
    result: *mut *mut Value,
    error: *mut *mut Error,
) -> i32 {
    let what = "lungo_invoke";
    let table: &'static TypeTable = unsafe { borrow(types, what) };
    unsafe {
        borrow(result, what);
        borrow(error, what);
    }
    let sig = Signature::decode(unsafe { bytes(sig, sig_len, what) })
        .unwrap_or_else(|e| lean_internal_panic(&format!("a generated signature is invalid: {e}")));
    let type_args: Vec<&TypeExpr> = if n_types == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(borrow(type_args, what), n_types) }
            .iter()
            .map(|t| unsafe { borrow(*t, what) })
            .collect()
    };
    let mut tys = Vec::new();
    for t in &type_args {
        join(Some(table), t.table, what);
        tys.push(t.ty.clone());
    }
    if tys.len() != sig.type_params as usize {
        misuse(what, &format!("{} type arguments for {} type parameters", tys.len(), sig.type_params));
    }
    let inst = sig.instantiate(&tys).unwrap_or_else(|e| misuse(what, &e.0));
    let args: Vec<&Value> = if n_args == 0 {
        Vec::new()
    } else {
        unsafe { std::slice::from_raw_parts(borrow(args, what), n_args) }
            .iter()
            .map(|a| unsafe { borrow(*a, what) })
            .collect()
    };
    let mut input = Vec::new();
    input.extend_from_slice(&(tys.len() as u32).to_le_bytes());
    for t in &tys {
        t.encode(&mut input);
    }
    if let Err(e) = encode_args(table, &inst.params, &args, &mut input) {
        return unsafe { complete(Err(malformed(e)), MALFORMED, result, error) };
    }
    let mut out = Buffer::empty();
    let status = unsafe { entry(input.as_ptr(), input.len(), &mut out) };
    let bytes = out.take();
    match status {
        OK => {
            let outcome = runtime_result(table, &inst.returns, &bytes, what);
            unsafe { complete(outcome, FAILED, result, error) }
        }
        MALFORMED => unsafe { complete(Err(malformed(String::from_utf8_lossy(&bytes))), MALFORMED, result, error) },
        s => lean_internal_panic(&format!("{what}: a generated entry point returned status {s}")),
    }
}

// ---------------------------------------------------------------------------------------------
// Functions
// ---------------------------------------------------------------------------------------------

unsafe fn function_type<'a>(t: *const TypeExpr, what: &str) -> (&'static TypeTable, &'a [Type], &'a Type) {
    let t = unsafe { borrow(t, what) };
    match &t.ty {
        Type::Function { params, result } => (t.table.unwrap_or_else(empty_table), params, result),
        _ => misuse(what, "the type is not a function type"),
    }
}

/// A host function of type `fn_type`: a call runs `f` with `ctx`; `drop` (if not `NULL`) frees
/// `ctx` once no value or Lean closure references the function. `f` may run on any thread.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_function(
    fn_type: *const TypeExpr,
    f: Option<CFunction>,
    ctx: *mut c_void,
    drop: Option<CDrop>,
) -> *mut Value {
    let what = "lungo_value_function";
    let (table, params, result) = unsafe { function_type(fn_type, what) };
    let f = f.unwrap_or_else(|| misuse(what, "the function is a null pointer"));
    let sig = Signature { type_params: 0, params: params.to_vec(), returns: Returns::Value(result.clone()) };
    give(Value::Function(FunctionRef::Host(host_register(table, sig, f, ctx, drop))))
}

/// Calls the function value `f` of type `fn_type` with `n` arguments (borrowed).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_value_call(
    f: *const Value,
    fn_type: *const TypeExpr,
    args: *const *const Value,
    n: usize,
    result: *mut *mut Value,
    error: *mut *mut Error,
) -> i32 {
    let what = "lungo_value_call";
    let (table, params, ret) = unsafe { function_type(fn_type, what) };
    unsafe {
        borrow(result, what);
        borrow(error, what);
    }
    let args_ptrs = if n == 0 { &[][..] } else { unsafe { std::slice::from_raw_parts(borrow(args, what), n) } };
    let values: Vec<&Value> = args_ptrs.iter().map(|a| unsafe { borrow(*a, what) }).collect();
    match unsafe { borrow(f, what) } {
        Value::Function(FunctionRef::Lean(h)) => {
            let mut input = 0u32.to_le_bytes().to_vec();
            if let Err(e) = encode_args(table, params, &values, &mut input) {
                return unsafe { complete(Err(malformed(e)), MALFORMED, result, error) };
            }
            let mut ty = Vec::new();
            Type::Function { params: params.to_vec(), result: Box::new(ret.clone()) }.encode(&mut ty);
            let mut out = Buffer::empty();
            let status = unsafe {
                super::boundary::lungo_closure_call(table, *h, ty.as_ptr(), ty.len(), input.as_ptr(), input.len(), &mut out)
            };
            let bytes = out.take();
            match status {
                OK => unsafe {
                    complete(runtime_result(table, &Returns::Value(ret.clone()), &bytes, what), FAILED, result, error)
                },
                _ => unsafe { complete(Err(malformed(String::from_utf8_lossy(&bytes))), MALFORMED, result, error) },
            }
        }
        Value::Function(FunctionRef::Host(id)) => {
            if values.len() != params.len() {
                let e = malformed(format!("{} arguments for {} parameters", values.len(), params.len()));
                return unsafe { complete(Err(e), MALFORMED, result, error) };
            }
            let entry = host_entry(*id);
            let ptrs: Vec<*const Value> = values.iter().map(|v| *v as *const Value).collect();
            match unsafe { run_host(&entry, &ptrs) } {
                Ok(v) => unsafe { complete(Ok(v), FAILED, result, error) },
                Err(e) => unsafe { complete(Err(e), FAILED, result, error) },
            }
        }
        other => wrong_kind(what, other),
    }
}

// ---------------------------------------------------------------------------------------------
// The C host
// ---------------------------------------------------------------------------------------------

struct HostEntry {
    table: &'static TypeTable,
    sig: Signature,
    f: CFunction,
    ctx: *mut c_void,
    drop: Option<CDrop>,
}

// The context belongs to the host function, which must be callable from any thread.
unsafe impl Send for HostEntry {}
unsafe impl Sync for HostEntry {}

impl Drop for HostEntry {
    fn drop(&mut self) {
        if let Some(d) = self.drop {
            unsafe { d(self.ctx) };
        }
    }
}

struct Registry {
    next: AtomicU64,
    entries: Mutex<HashMap<u64, (Arc<HostEntry>, usize)>>,
}

fn registry() -> &'static Registry {
    static R: OnceLock<Registry> = OnceLock::new();
    R.get_or_init(|| Registry { next: AtomicU64::new(1), entries: Mutex::new(HashMap::new()) })
}

fn install_host() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| wire::set_host(Host { dispatch: c_dispatch, retain: c_retain, release: c_release }));
}

/// Registers a host function with one reference, owned by the caller.
fn host_register(table: &'static TypeTable, sig: Signature, f: CFunction, ctx: *mut c_void, drop: Option<CDrop>) -> u64 {
    install_host();
    let r = registry();
    let id = r.next.fetch_add(1, Ordering::Relaxed);
    let entry = Arc::new(HostEntry { table, sig, f, ctx, drop });
    r.entries.lock().unwrap_or_else(|p| p.into_inner()).insert(id, (entry, 1));
    id
}

fn host_entry(id: u64) -> Arc<HostEntry> {
    let entries = registry().entries.lock().unwrap_or_else(|p| p.into_inner());
    match entries.get(&id) {
        Some((e, _)) => e.clone(),
        None => lean_internal_panic(&format!("host function {id} is not registered (was its value freed?)")),
    }
}

fn host_retain(id: u64) {
    let mut entries = registry().entries.lock().unwrap_or_else(|p| p.into_inner());
    match entries.get_mut(&id) {
        Some((_, refs)) => *refs += 1,
        None => lean_internal_panic(&format!("host function {id} is not registered (was its value freed?)")),
    }
}

fn host_release(id: u64) {
    let removed = {
        let mut entries = registry().entries.lock().unwrap_or_else(|p| p.into_inner());
        match entries.get_mut(&id) {
            Some((_, refs)) if *refs > 1 => {
                *refs -= 1;
                None
            }
            Some(_) => entries.remove(&id),
            None => lean_internal_panic(&format!("host function {id} is released more often than it is referenced")),
        }
    };
    // The context is freed outside the lock: its `drop` may free values holding host functions.
    drop(removed);
}

unsafe extern "C" fn c_retain(id: u64) {
    host_retain(id);
}

unsafe extern "C" fn c_release(id: u64) {
    host_release(id);
}

/// Runs a host function with `args` (borrowed): its result, or its error.
unsafe fn run_host(entry: &HostEntry, args: &[*const Value]) -> Result<Value, Error> {
    let mut result: *mut Value = std::ptr::null_mut();
    let mut error: *mut Error = std::ptr::null_mut();
    let status = unsafe { (entry.f)(entry.ctx, args.as_ptr(), args.len(), &mut result, &mut error) };
    let taken = |p: *mut Value| if p.is_null() { None } else { Some(*unsafe { Box::from_raw(p) }) };
    let taken_error = |p: *mut Error| if p.is_null() { None } else { Some(*unsafe { Box::from_raw(p) }) };
    let (result, error) = (taken(result), taken_error(error));
    let fail = |msg: &str, result: Option<Value>, error: Option<Error>| -> ! {
        if let Some(v) = result {
            free_value(v);
        }
        if let Some(e) = error {
            unsafe { lungo_error_free(give(e)) };
        }
        lean_internal_panic(&format!("a C host function {msg}"))
    };
    match (status, result, error) {
        (OK, Some(v), None) => Ok(v),
        (FAILED, None, Some(e)) => Err(e),
        (OK, r, e) => fail("returned LUNGO_OK without exactly a result", r, e),
        (FAILED, r, e) => fail("returned LUNGO_FAILED without exactly an error", r, e),
        (s, r, e) => fail(&format!("returned status {s} (LUNGO_OK or LUNGO_FAILED expected)"), r, e),
    }
}

/// Lean calls host function `id`: decodes the arguments, runs the function, encodes its result.
unsafe extern "C" fn c_dispatch(id: u64, input: *const u8, len: usize, out: *mut Buffer) -> i32 {
    let entry = host_entry(id);
    let input = unsafe { bytes(input, len, "c_dispatch") };
    let mut r = Reader::new(input);
    let mut args = Vec::new();
    for p in &entry.sig.params {
        match wv::decode(entry.table, p, &mut r) {
            Ok(v) => args.push(v),
            Err(e) => lean_internal_panic(&format!("Lean passed malformed arguments to a C host function: {e}")),
        }
    }
    if let Err(e) = r.finish() {
        lean_internal_panic(&format!("Lean passed malformed arguments to a C host function: {e}"));
    }
    let ptrs: Vec<*const Value> = args.iter().map(|v| v as *const Value).collect();
    let outcome = unsafe { run_host(&entry, &ptrs) };
    args.into_iter().for_each(free_value);
    // The result transfers its handles to Lean; the host references in it are Lean's to retain.
    let mut bytes = Vec::new();
    let encoded: Result<(), String> = match (&entry.sig.returns, outcome) {
        (Returns::Value(t), Ok(v)) => {
            let r = wv::encode(entry.table, t, &v, &mut bytes).map_err(|e| format!("returned a malformed value: {e}"));
            transfer(v);
            r
        }
        (Returns::Io(t) | Returns::Eio { value: t, .. }, Ok(v)) => {
            bytes.push(result::OK);
            let r = wv::encode(entry.table, t, &v, &mut bytes).map_err(|e| format!("returned a malformed value: {e}"));
            transfer(v);
            r
        }
        (Returns::Value(_), Err(e)) => {
            let msg = e.message.to_string_lossy().into_owned();
            unsafe { lungo_error_free(give(e)) };
            Err(msg)
        }
        (Returns::Io(_), Err(e)) => match e.data {
            ErrorData::Io(h) => {
                bytes.push(result::ERROR);
                bytes.extend_from_slice(&h.to_le_bytes());
                let text = e.message.to_bytes();
                bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
                bytes.extend_from_slice(text);
                Ok(())
            }
            _ => lean_internal_panic("a C host function of an IO extern raised an error that is not an IO error"),
        },
        (Returns::Eio { error, .. }, Err(e)) => match e.data {
            ErrorData::Value(v) => {
                bytes.push(result::ERROR);
                let r = wv::encode(entry.table, error, &v, &mut bytes).map_err(|e| format!("returned a malformed error: {e}"));
                transfer(v);
                r
            }
            _ => lean_internal_panic("a C host function of an EIO extern raised an error that is not an error value"),
        },
    };
    let out = unsafe { &mut *out };
    match encoded {
        Ok(()) => {
            out.set(bytes);
            0
        }
        Err(msg) => {
            out.set(msg.into_bytes());
            1
        }
    }
}

/// Gives up `v`, whose handles now belong to the runtime, releasing its host references.
fn transfer(v: Value) {
    match v {
        Value::Function(FunctionRef::Host(id)) => host_release(id),
        Value::Option(Some(x)) => transfer(*x),
        Value::List(xs) | Value::Array(xs) => xs.into_iter().for_each(transfer),
        Value::Ctor { fields, .. } => fields.into_iter().for_each(transfer),
        Value::Prod(a, b) => {
            transfer(*a);
            transfer(*b);
        }
        Value::Except(Ok(x)) | Value::Except(Err(x)) => transfer(*x),
        _ => {}
    }
}

/// Registers the C function `f` as the implementation of a host extern of the generated
/// signature `sig` in program `types`, and returns its callback identifier for
/// `<prefix>set_host_extern`. The registration lasts for the life of the process.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn lungo_host_function_new(
    types: *const TypeTable,
    sig: *const u8,
    sig_len: usize,
    f: Option<CFunction>,
    ctx: *mut c_void,
    drop: Option<CDrop>,
) -> u64 {
    let what = "lungo_host_function_new";
    let table: &'static TypeTable = unsafe { borrow(types, what) };
    let f = f.unwrap_or_else(|| misuse(what, "the function is a null pointer"));
    let sig = Signature::decode(unsafe { bytes(sig, sig_len, what) })
        .and_then(|s| s.check(table).map(|_| s))
        .unwrap_or_else(|e| lean_internal_panic(&format!("a generated signature is invalid: {e}")));
    if sig.type_params != 0 {
        lean_internal_panic("a host extern's generated signature has type parameters");
    }
    host_register(table, sig, f, ctx, drop)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::*;
    use std::sync::atomic::AtomicUsize;

    fn nat(v: *const Value) -> u64 {
        let mut x = 0;
        assert!(unsafe { lungo_value_get_nat(v, &mut x) });
        x
    }

    fn nat_type() -> *mut TypeExpr {
        lungo_type_simple(tag::NAT)
    }

    /// `Nat → Nat` as a type expression.
    fn nat_to_nat() -> *mut TypeExpr {
        let n = nat_type();
        let t = unsafe { lungo_type_function(&(n as *const TypeExpr), 1, n) };
        unsafe { lungo_type_free(n) };
        t
    }

    #[test]
    fn invalid_data_is_rejected_at_construction() {
        unsafe {
            assert!(lungo_value_nat_parse(c"12a".as_ptr()).is_null());
            assert!(lungo_value_nat_parse(c"".as_ptr()).is_null());
            assert!(lungo_value_nat_parse(c"-1".as_ptr()).is_null());
            assert!(lungo_value_int_parse(c"-".as_ptr()).is_null());
            assert!(lungo_value_int_parse(c"+5".as_ptr()).is_null());
            assert!(lungo_value_char(0xd800).is_null());
            assert!(lungo_value_char(0x110000).is_null());
            assert!(lungo_value_string(b"\xff".as_ptr() as *const c_char, 1).is_null());
            let big = lungo_value_nat_parse(c"18446744073709551616".as_ptr());
            let mut x = 0;
            assert!(!lungo_value_get_nat(big, &mut x), "2^64 does not fit 64 bits");
            let s = lungo_value_number_string(big);
            assert_eq!(CStr::from_ptr(s).to_str().unwrap(), "18446744073709551616");
            lungo_string_free(s);
            lungo_value_free(big);
            let neg = lungo_value_int_parse(c"-42".as_ptr());
            let mut i = 0;
            assert!(lungo_value_get_int(neg, &mut i));
            assert_eq!(i, -42);
            lungo_value_free(neg);
            let nul = lungo_value_string(b"a\0b".as_ptr() as *const c_char, 3);
            assert!(lungo_value_string_dup(nul).is_null(), "a string with NUL has no C string");
            let mut len = 0;
            let p = lungo_value_get_string(nul, &mut len);
            assert_eq!(std::slice::from_raw_parts(p as *const u8, len), b"a\0b");
            lungo_value_free(nul);
        }
    }

    #[test]
    fn values_own_their_handles() {
        unsafe {
            let h = wire::handle_new(lean_mk_string("owned"));
            let v = give(Value::Opaque(h));
            let c = lungo_value_clone(v);
            let Value::Opaque(hc) = &*c else { unreachable!() };
            assert_ne!(*hc, h, "a clone owns a handle of its own");
            assert!(!lungo_value_equal(v, c), "values are equal only with the same handle");
            let hc = *hc;
            lungo_value_free(v);
            assert!(wire::handle_get(h).is_err(), "freeing a value releases its handle");
            let o = wire::handle_get(hc).unwrap();
            assert_eq!(lean_string_bytes(o), b"owned");
            lean_dec(o);
            lungo_value_free(c);
            assert!(wire::handle_get(hc).is_err());
        }
    }

    static DROPPED: AtomicUsize = AtomicUsize::new(0);

    unsafe extern "C" fn add(
        ctx: *mut c_void,
        args: *const *const Value,
        n: usize,
        result: *mut *mut Value,
        error: *mut *mut Error,
    ) -> i32 {
        unsafe {
            assert_eq!(n, 1);
            let k = *(ctx as *const u64);
            let x = nat(*args);
            if x == 0 {
                *error = lungo_error_io(c"zero".as_ptr());
                return FAILED;
            }
            *result = lungo_value_nat(x + k);
            OK
        }
    }

    unsafe extern "C" fn drop_ctx(ctx: *mut c_void) {
        drop(unsafe { Box::from_raw(ctx as *mut u64) });
        DROPPED.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn host_functions_live_while_referenced_by_values_or_lean() {
        unsafe {
            let ty = nat_to_nat();
            let before = DROPPED.load(Ordering::SeqCst);
            let f = lungo_value_function(ty, Some(add), Box::into_raw(Box::new(10u64)) as *mut c_void, Some(drop_ctx));
            // The host function called from C.
            let three = lungo_value_nat(3);
            let (mut r, mut e) = (std::ptr::null_mut(), std::ptr::null_mut());
            assert_eq!(lungo_value_call(f, ty, &(three as *const Value), 1, &mut r, &mut e), OK);
            assert_eq!(nat(r), 13);
            lungo_value_free(r);
            // Its failure is the function's error.
            let zero = lungo_value_nat(0);
            assert_eq!(lungo_value_call(f, ty, &(zero as *const Value), 1, &mut r, &mut e), FAILED);
            assert!(r.is_null());
            assert_eq!(lungo_error_kind(e), error_kind::IO);
            assert_eq!(CStr::from_ptr(lungo_error_message(e)).to_str().unwrap(), "zero");
            lungo_error_free(e);
            lungo_value_free(zero);

            // Passed to Lean: the runtime's closure retains the function and calls it through the
            // C host.
            let mut bytes = Vec::new();
            wv::encode(empty_table(), &(*ty).ty, &*f, &mut bytes).unwrap();
            let mut rd = Reader::new(&bytes);
            let closure = wire::decode(empty_table(), &(*ty).ty, &mut rd, wire::Handles::Borrow).unwrap();
            lungo_value_free(f);
            assert_eq!(DROPPED.load(Ordering::SeqCst), before, "Lean's closure keeps the function alive");
            let out = crate::apply::lean_apply_n(closure, &[crate::nat::nat_from_biguint(BigUint::from(5u32))]);
            assert_eq!(crate::nat::nat_to_bigint(out), BigInt::from(15));
            lean_dec(out);
            lean_dec(closure);
            assert_eq!(DROPPED.load(Ordering::SeqCst), before + 1, "freeing the last reference frees the context");
            lungo_value_free(three);
            lungo_type_free(ty);
        }
    }

    unsafe extern "C" fn raise_received(
        ctx: *mut c_void,
        args: *const *const Value,
        _n: usize,
        _result: *mut *mut Value,
        error: *mut *mut Error,
    ) -> i32 {
        unsafe {
            let received = &*(ctx as *const Error);
            let _ = args;
            *error = lungo_error_clone(received);
            FAILED
        }
    }

    #[test]
    fn io_errors_raised_by_hosts_transfer_their_handle_to_lean() {
        unsafe {
            // An error object as Lean sends it: a new handle and the message. (Any object stands
            // for the `IO.Error`, whose rendering needs a compiled program.)
            let lean_error = lean_mk_string("from Lean");
            lean_inc(lean_error);
            let mut bytes = wire::handle_new(lean_error).to_le_bytes().to_vec();
            bytes.extend_from_slice(&9u32.to_le_bytes());
            bytes.extend_from_slice(b"from Lean");
            let received = decode_result(empty_table(), &Returns::Io(Type::Unit), &[&[result::ERROR][..], &bytes].concat())
                .unwrap()
                .unwrap_err();
            let ErrorData::Io(h) = received.data else { unreachable!() };
            let received = give(received);
            let sig = Signature { type_params: 0, params: vec![Type::Unit], returns: Returns::Io(Type::Unit) };
            let id = host_register(empty_table(), sig, raise_received, received as *mut c_void, None);
            let mut input = Vec::new();
            let mut out = Buffer::empty();
            assert_eq!(c_dispatch(id, input.as_mut_ptr(), 0, &mut out), 0);
            let reply = out.take();
            let mut r = Reader::new(&reply);
            assert_eq!(r.u8().unwrap(), result::ERROR);
            let raised = wire::decode_io_error(&mut r).unwrap();
            assert_eq!(raised, lean_error, "the host re-raised Lean's own error");
            lean_dec(raised);
            lungo_error_free(received);
            assert!(wire::handle_get(h).is_err(), "the received error's handle was released with it");
            host_release(id);
            lean_dec(lean_error);
        }
    }

    #[test]
    fn arguments_that_do_not_match_the_signature_are_malformed() {
        unsafe {
            let ty = nat_to_nat();
            let f = lungo_value_function(ty, Some(add), Box::into_raw(Box::new(1u64)) as *mut c_void, Some(drop_ctx));
            let s = lungo_value_cstring(c"not a Nat".as_ptr());
            let mut bytes = Vec::new();
            let err = encode_args(empty_table(), &[Type::Nat], &[&*s], &mut bytes).unwrap_err();
            assert!(err.0.contains("argument 1"), "{err}");
            let (mut r, mut e) = (std::ptr::null_mut(), std::ptr::null_mut());
            let none: [*const Value; 0] = [];
            assert_eq!(lungo_value_call(f, ty, none.as_ptr(), 0, &mut r, &mut e), MALFORMED);
            assert_eq!(lungo_error_kind(e), error_kind::MALFORMED);
            lungo_error_free(e);
            lungo_value_free(s);
            lungo_value_free(f);
            lungo_type_free(ty);
        }
    }
}
