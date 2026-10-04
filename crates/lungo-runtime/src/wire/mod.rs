//! The wire format: how values cross between a Lean program and the language bindings that
//! call it (C, Go, Python, Swift, TypeScript).
//!
//! A value is encoded according to its *type expression*; a program describes its inductive
//! types in a *type table*. Both are binary and specified in `reference/wire-format.md` of the
//! documentation; the constants of this module are that specification's single source, which
//! the code generators use and the bindings' support libraries are tested against.
//!
//! Decoding builds Lean objects in their boxed representation, exactly as the Rust facade's
//! `LeanType` conversions do; encoding reads them. Every read is bounds-checked and every value
//! validated: malformed input is a [`WireError`], never undefined behaviour.

pub mod value;

use crate::object::*;
use num_bigint::{BigInt, BigUint, Sign};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

/// Tags of type expressions.
pub mod tag {
    pub const NAT: u8 = 0;
    pub const INT: u8 = 1;
    pub const BOOL: u8 = 2;
    pub const UINT8: u8 = 3;
    pub const UINT16: u8 = 4;
    pub const UINT32: u8 = 5;
    pub const UINT64: u8 = 6;
    pub const USIZE: u8 = 7;
    pub const INT8: u8 = 8;
    pub const INT16: u8 = 9;
    pub const INT32: u8 = 10;
    pub const INT64: u8 = 11;
    pub const ISIZE: u8 = 12;
    pub const FLOAT: u8 = 13;
    pub const FLOAT32: u8 = 14;
    pub const CHAR: u8 = 15;
    pub const STRING: u8 = 16;
    pub const UNIT: u8 = 17;
    pub const BYTE_ARRAY: u8 = 18;
    pub const FLOAT_ARRAY: u8 = 19;
    pub const OPTION: u8 = 20;
    pub const LIST: u8 = 21;
    pub const ARRAY: u8 = 22;
    pub const PROD: u8 = 23;
    pub const EXCEPT: u8 = 24;
    pub const FUNCTION: u8 = 25;
    pub const PARAM: u8 = 26;
    pub const INDUCTIVE: u8 = 27;
    pub const OPAQUE: u8 = 28;
}

/// Codes of the runtime representations (`IrType`) in type tables.
pub mod repr {
    pub const FLOAT: u8 = 0;
    pub const FLOAT32: u8 = 1;
    pub const UINT8: u8 = 2;
    pub const UINT16: u8 = 3;
    pub const UINT32: u8 = 4;
    pub const UINT64: u8 = 5;
    pub const USIZE: u8 = 6;
    pub const OBJECT: u8 = 7;
}

/// Codes of the kinds of constructor fields in type tables.
pub mod field {
    pub const OBJECT: u8 = 0;
    pub const USIZE: u8 = 1;
    pub const SCALAR: u8 = 2;
}

/// The first bytes of a type table.
pub const TABLE_MAGIC: &[u8; 4] = b"LNGT";
/// The version of the type-table and type-expression encoding.
pub const TABLE_VERSION: u32 = 2;

/// How a `Function` value is passed.
pub mod function {
    /// A closure of the program, by handle.
    pub const LEAN: u8 = 0;
    /// A function of the host, by callback identifier.
    pub const HOST: u8 = 1;
}

/// Tags of `IO` and `EIO` results.
pub mod result {
    pub const OK: u8 = 0;
    pub const ERROR: u8 = 1;
}

/// Why wire data was rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WireError(pub String);

impl std::fmt::Display for WireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for WireError {}

fn err<T>(msg: impl Into<String>) -> Result<T, WireError> {
    Err(WireError(msg.into()))
}

// ---------------------------------------------------------------------------------------------
// Type expressions and type tables
// ---------------------------------------------------------------------------------------------

/// A type expression.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Type {
    Nat,
    Int,
    Bool,
    #[cfg_attr(feature = "serde", serde(rename = "uint8"))]
    UInt8,
    #[cfg_attr(feature = "serde", serde(rename = "uint16"))]
    UInt16,
    #[cfg_attr(feature = "serde", serde(rename = "uint32"))]
    UInt32,
    #[cfg_attr(feature = "serde", serde(rename = "uint64"))]
    UInt64,
    #[cfg_attr(feature = "serde", serde(rename = "usize"))]
    USize,
    Int8,
    Int16,
    Int32,
    Int64,
    #[cfg_attr(feature = "serde", serde(rename = "isize"))]
    ISize,
    Float,
    Float32,
    Char,
    String,
    Unit,
    ByteArray,
    FloatArray,
    Option(Box<Type>),
    List(Box<Type>),
    Array(Box<Type>),
    Prod(Box<Type>, Box<Type>),
    Except {
        error: Box<Type>,
        value: Box<Type>,
    },
    Function {
        params: Vec<Type>,
        result: Box<Type>,
    },
    /// The type parameter at this index of the enclosing type or signature.
    Param(u32),
    /// The type at this index of the program's type table, applied to arguments.
    Inductive {
        index: u32,
        args: Vec<Type>,
    },
    Opaque,
}

/// The largest number of parameters of a function value crossing the boundary.
pub const MAX_FUNCTION_PARAMS: usize = 15;

impl Type {
    /// Appends the encoding of the type expression.
    pub fn encode(&self, out: &mut Vec<u8>) {
        let simple = |out: &mut Vec<u8>, t: u8| out.push(t);
        match self {
            Type::Nat => simple(out, tag::NAT),
            Type::Int => simple(out, tag::INT),
            Type::Bool => simple(out, tag::BOOL),
            Type::UInt8 => simple(out, tag::UINT8),
            Type::UInt16 => simple(out, tag::UINT16),
            Type::UInt32 => simple(out, tag::UINT32),
            Type::UInt64 => simple(out, tag::UINT64),
            Type::USize => simple(out, tag::USIZE),
            Type::Int8 => simple(out, tag::INT8),
            Type::Int16 => simple(out, tag::INT16),
            Type::Int32 => simple(out, tag::INT32),
            Type::Int64 => simple(out, tag::INT64),
            Type::ISize => simple(out, tag::ISIZE),
            Type::Float => simple(out, tag::FLOAT),
            Type::Float32 => simple(out, tag::FLOAT32),
            Type::Char => simple(out, tag::CHAR),
            Type::String => simple(out, tag::STRING),
            Type::Unit => simple(out, tag::UNIT),
            Type::ByteArray => simple(out, tag::BYTE_ARRAY),
            Type::FloatArray => simple(out, tag::FLOAT_ARRAY),
            Type::Opaque => simple(out, tag::OPAQUE),
            Type::Option(t) => {
                out.push(tag::OPTION);
                t.encode(out);
            }
            Type::List(t) => {
                out.push(tag::LIST);
                t.encode(out);
            }
            Type::Array(t) => {
                out.push(tag::ARRAY);
                t.encode(out);
            }
            Type::Prod(a, b) => {
                out.push(tag::PROD);
                a.encode(out);
                b.encode(out);
            }
            Type::Except { error, value } => {
                out.push(tag::EXCEPT);
                error.encode(out);
                value.encode(out);
            }
            Type::Function { params, result } => {
                out.push(tag::FUNCTION);
                out.extend_from_slice(&(params.len() as u32).to_le_bytes());
                for p in params {
                    p.encode(out);
                }
                result.encode(out);
            }
            Type::Param(i) => {
                out.push(tag::PARAM);
                out.extend_from_slice(&i.to_le_bytes());
            }
            Type::Inductive { index, args } => {
                out.push(tag::INDUCTIVE);
                out.extend_from_slice(&index.to_le_bytes());
                out.extend_from_slice(&(args.len() as u32).to_le_bytes());
                for a in args {
                    a.encode(out);
                }
            }
        }
    }

    /// The type with its parameters replaced by `args`, which contain no parameters.
    pub fn substitute(&self, args: &[Type]) -> Result<Type, WireError> {
        Ok(match self {
            Type::Param(i) => match args.get(*i as usize) {
                Some(t) => t.clone(),
                None => return err(format!("type parameter {i} of {} is not instantiated", args.len())),
            },
            Type::Option(t) => Type::Option(Box::new(t.substitute(args)?)),
            Type::List(t) => Type::List(Box::new(t.substitute(args)?)),
            Type::Array(t) => Type::Array(Box::new(t.substitute(args)?)),
            Type::Prod(a, b) => Type::Prod(Box::new(a.substitute(args)?), Box::new(b.substitute(args)?)),
            Type::Except { error, value } => {
                Type::Except { error: Box::new(error.substitute(args)?), value: Box::new(value.substitute(args)?) }
            }
            Type::Function { params, result } => Type::Function {
                params: params.iter().map(|p| p.substitute(args)).collect::<Result<_, _>>()?,
                result: Box::new(result.substitute(args)?),
            },
            Type::Inductive { index, args: a } => {
                Type::Inductive { index: *index, args: a.iter().map(|t| t.substitute(args)).collect::<Result<_, _>>()? }
            }
            other => other.clone(),
        })
    }
}

/// What a function returns across the boundary.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case", deny_unknown_fields))]
pub enum Returns {
    /// A value of the type.
    Value(Type),
    /// `IO α`: the value, or an `IO.Error`.
    Io(Type),
    /// `EIO ε α`: the value, or an error of `ε`.
    Eio { error: Type, value: Type },
}

/// Kinds of [`Returns`] in an encoded [`Signature`].
pub mod returns {
    pub const VALUE: u8 = 0;
    pub const IO: u8 = 1;
    pub const EIO: u8 = 2;
}

/// The signature of a function crossing the boundary: its number of type parameters (which its
/// types refer to with [`Type::Param`]), its parameters, and what it returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Signature {
    pub type_params: u32,
    pub params: Vec<Type>,
    pub returns: Returns,
}

impl Signature {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        put_u32(&mut out, self.type_params);
        put_len(&mut out, self.params.len());
        for p in &self.params {
            p.encode(&mut out);
        }
        match &self.returns {
            Returns::Value(t) => {
                out.push(returns::VALUE);
                t.encode(&mut out);
            }
            Returns::Io(t) => {
                out.push(returns::IO);
                t.encode(&mut out);
            }
            Returns::Eio { error, value } => {
                out.push(returns::EIO);
                error.encode(&mut out);
                value.encode(&mut out);
            }
        }
        out
    }

    pub fn decode(bytes: &[u8]) -> Result<Signature, WireError> {
        let mut r = Reader::new(bytes);
        let type_params = r.u32()?;
        let n = r.count(1)?;
        let params = (0..n).map(|_| r.ty()).collect::<Result<_, _>>()?;
        let returns = match r.u8()? {
            returns::VALUE => Returns::Value(r.ty()?),
            returns::IO => Returns::Io(r.ty()?),
            returns::EIO => Returns::Eio { error: r.ty()?, value: r.ty()? },
            k => return err(format!("invalid result kind {k}")),
        };
        r.finish()?;
        Ok(Signature { type_params, params, returns })
    }

    /// Checks that the signature's types are valid in `table`.
    pub fn check(&self, table: &TypeTable) -> Result<(), WireError> {
        for p in &self.params {
            table.check_type(p, self.type_params)?;
        }
        match &self.returns {
            Returns::Value(t) | Returns::Io(t) => table.check_type(t, self.type_params),
            Returns::Eio { error, value } => {
                table.check_type(error, self.type_params)?;
                table.check_type(value, self.type_params)
            }
        }
    }

    /// The signature with its type parameters instantiated by `args`.
    pub fn instantiate(&self, args: &[Type]) -> Result<Signature, WireError> {
        if args.len() != self.type_params as usize {
            return err(format!("{} type arguments for {} type parameters", args.len(), self.type_params));
        }
        Ok(Signature {
            type_params: 0,
            params: self.params.iter().map(|p| p.substitute(args)).collect::<Result<_, _>>()?,
            returns: match &self.returns {
                Returns::Value(t) => Returns::Value(t.substitute(args)?),
                Returns::Io(t) => Returns::Io(t.substitute(args)?),
                Returns::Eio { error, value } => {
                    Returns::Eio { error: error.substitute(args)?, value: value.substitute(args)? }
                }
            },
        })
    }
}

/// The runtime representation of a type table entry or scalar field.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum Repr {
    Float,
    Float32,
    #[cfg_attr(feature = "serde", serde(rename = "uint8"))]
    UInt8,
    #[cfg_attr(feature = "serde", serde(rename = "uint16"))]
    UInt16,
    #[cfg_attr(feature = "serde", serde(rename = "uint32"))]
    UInt32,
    #[cfg_attr(feature = "serde", serde(rename = "uint64"))]
    UInt64,
    #[cfg_attr(feature = "serde", serde(rename = "usize"))]
    USize,
    Object,
}

impl Repr {
    pub fn code(self) -> u8 {
        match self {
            Repr::Float => repr::FLOAT,
            Repr::Float32 => repr::FLOAT32,
            Repr::UInt8 => repr::UINT8,
            Repr::UInt16 => repr::UINT16,
            Repr::UInt32 => repr::UINT32,
            Repr::UInt64 => repr::UINT64,
            Repr::USize => repr::USIZE,
            Repr::Object => repr::OBJECT,
        }
    }

    fn of_code(c: u8) -> Result<Repr, WireError> {
        Ok(match c {
            repr::FLOAT => Repr::Float,
            repr::FLOAT32 => Repr::Float32,
            repr::UINT8 => Repr::UInt8,
            repr::UINT16 => Repr::UInt16,
            repr::UINT32 => Repr::UInt32,
            repr::UINT64 => Repr::UInt64,
            repr::USIZE => Repr::USize,
            repr::OBJECT => Repr::Object,
            _ => return err(format!("unknown representation code {c}")),
        })
    }
}

/// How a constructor field is stored.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", serde(rename_all = "snake_case"))]
pub enum FieldKind {
    /// The object field at this index.
    Object(u32),
    /// The `usize` field at this index (counted in words, after the object fields).
    #[cfg_attr(feature = "serde", serde(rename = "usize"))]
    USize(u32),
    /// A scalar at `offset` bytes after the object and `usize` fields.
    Scalar { offset: u32, repr: Repr },
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub kind: FieldKind,
    pub ty: Type,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ctor {
    pub name: String,
    pub tag: u32,
    /// Object fields.
    pub size: u32,
    /// `usize` fields.
    pub usize: u32,
    /// Bytes of other scalar fields.
    pub ssize: u32,
    pub fields: Vec<Field>,
}

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeDecl {
    pub name: String,
    /// A type whose values cross only as handles (they carry proofs, or are otherwise not
    /// first-order data): named, so bindings give it a type of its own, but with no parameters
    /// and no constructors.
    pub opaque: bool,
    pub params: u32,
    pub repr: Repr,
    /// For a single-constructor type represented by one field: that constructor and field.
    pub trivial: Option<(u32, u32)>,
    pub ctors: Vec<Ctor>,
}

/// A program's inductive types.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct TypeTable {
    pub types: Vec<TypeDecl>,
}

impl TypeTable {
    /// The table's encoding.
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(TABLE_MAGIC);
        out.extend_from_slice(&TABLE_VERSION.to_le_bytes());
        put_u32(&mut out, self.types.len() as u32);
        for t in &self.types {
            put_str(&mut out, &t.name);
            out.push(t.opaque as u8);
            put_u32(&mut out, t.params);
            out.push(t.repr.code());
            match t.trivial {
                Some((c, f)) => {
                    out.push(1);
                    put_u32(&mut out, c);
                    put_u32(&mut out, f);
                }
                None => out.push(0),
            }
            put_u32(&mut out, t.ctors.len() as u32);
            for c in &t.ctors {
                put_str(&mut out, &c.name);
                put_u32(&mut out, c.tag);
                put_u32(&mut out, c.size);
                put_u32(&mut out, c.usize);
                put_u32(&mut out, c.ssize);
                put_u32(&mut out, c.fields.len() as u32);
                for f in &c.fields {
                    put_str(&mut out, &f.name);
                    match f.kind {
                        FieldKind::Object(i) => {
                            out.push(field::OBJECT);
                            put_u32(&mut out, i);
                        }
                        FieldKind::USize(i) => {
                            out.push(field::USIZE);
                            put_u32(&mut out, i);
                        }
                        FieldKind::Scalar { offset, repr } => {
                            out.push(field::SCALAR);
                            put_u32(&mut out, offset);
                            out.push(repr.code());
                        }
                    }
                    f.ty.encode(&mut out);
                }
            }
        }
        out
    }

    /// Parses and validates an encoded table.
    pub fn decode(bytes: &[u8]) -> Result<TypeTable, WireError> {
        let mut r = Reader::new(bytes);
        if r.take(4)? != TABLE_MAGIC {
            return err("not a lungo type table");
        }
        let version = r.u32()?;
        if version != TABLE_VERSION {
            return err(format!("type table version {version}; this runtime reads version {TABLE_VERSION}"));
        }
        let n = r.u32()?;
        let mut types = Vec::new();
        for _ in 0..n {
            let name = r.string()?;
            let opaque = match r.u8()? {
                0 => false,
                1 => true,
                b => return err(format!("invalid opaque flag {b} in {name}")),
            };
            let params = r.u32()?;
            let repr = Repr::of_code(r.u8()?)?;
            let trivial = match r.u8()? {
                0 => None,
                1 => Some((r.u32()?, r.u32()?)),
                b => return err(format!("invalid trivial-structure flag {b} in {name}")),
            };
            let nctors = r.u32()?;
            let mut ctors = Vec::new();
            for _ in 0..nctors {
                let cname = r.string()?;
                let (tag, size, usize, ssize) = (r.u32()?, r.u32()?, r.u32()?, r.u32()?);
                let nfields = r.u32()?;
                let mut fields = Vec::new();
                for _ in 0..nfields {
                    let fname = r.string()?;
                    let kind = match r.u8()? {
                        field::OBJECT => FieldKind::Object(r.u32()?),
                        field::USIZE => FieldKind::USize(r.u32()?),
                        field::SCALAR => FieldKind::Scalar { offset: r.u32()?, repr: Repr::of_code(r.u8()?)? },
                        k => return err(format!("invalid field kind {k} in {cname}")),
                    };
                    fields.push(Field { name: fname, kind, ty: r.ty()? });
                }
                ctors.push(Ctor { name: cname, tag, size, usize, ssize, fields });
            }
            types.push(TypeDecl { name, opaque, params, repr, trivial, ctors });
        }
        r.finish()?;
        let table = TypeTable { types };
        table.validate()?;
        Ok(table)
    }

    fn validate(&self) -> Result<(), WireError> {
        for t in &self.types {
            if t.opaque {
                if !t.ctors.is_empty() || t.params != 0 || t.trivial.is_some() || t.repr != Repr::Object {
                    return err(format!(
                        "opaque type {} must have no constructors, no parameters and an object representation",
                        t.name
                    ));
                }
                continue;
            }
            if t.ctors.is_empty() {
                return err(format!("type {} has no constructors", t.name));
            }
            if let Some((c, f)) = t.trivial {
                let ok = t.ctors.get(c as usize).is_some_and(|c| (f as usize) < c.fields.len());
                if !ok || t.ctors.len() != 1 {
                    return err(format!("type {} has an invalid trivial-structure field", t.name));
                }
            }
            for c in &t.ctors {
                if c.tag > LEAN_MAX_CTOR_TAG as u32 || c.size >= LEAN_MAX_CTOR_FIELDS {
                    return err(format!("constructor {} has an invalid layout", c.name));
                }
                for f in &c.fields {
                    match f.kind {
                        FieldKind::Object(i) if i >= c.size => {
                            return err(format!("field {} of {} is outside its object fields", f.name, c.name));
                        }
                        FieldKind::USize(i) if i >= c.usize => {
                            return err(format!("field {} of {} is outside its usize fields", f.name, c.name));
                        }
                        FieldKind::Scalar { offset, repr } => {
                            let width = scalar_width(repr)
                                .ok_or_else(|| WireError(format!("field {} of {} is not a scalar", f.name, c.name)))?;
                            if offset as usize + width > c.ssize as usize {
                                return err(format!("field {} of {} is outside its scalar fields", f.name, c.name));
                            }
                        }
                        _ => {}
                    }
                    self.check_type(&f.ty, t.params)?;
                }
            }
        }
        Ok(())
    }

    /// Checks that `ty` refers only to this table's types, with their arities, and to
    /// parameters below `params`.
    pub fn check_type(&self, ty: &Type, params: u32) -> Result<(), WireError> {
        match ty {
            Type::Param(i) if *i >= params => err(format!("type parameter {i} of {params}")),
            Type::Option(t) | Type::List(t) | Type::Array(t) => self.check_type(t, params),
            Type::Prod(a, b) | Type::Except { error: a, value: b } => {
                self.check_type(a, params)?;
                self.check_type(b, params)
            }
            Type::Function { params: ps, result } => {
                if ps.is_empty() || ps.len() > MAX_FUNCTION_PARAMS {
                    return err(format!("a function type with {} parameters", ps.len()));
                }
                for p in ps {
                    self.check_type(p, params)?;
                }
                self.check_type(result, params)
            }
            Type::Inductive { index, args } => {
                let decl = self
                    .types
                    .get(*index as usize)
                    .ok_or_else(|| WireError(format!("type index {index} outside the type table")))?;
                if args.len() != decl.params as usize {
                    return err(format!("{} applied to {} arguments", decl.name, args.len()));
                }
                args.iter().try_for_each(|a| self.check_type(a, params))
            }
            _ => Ok(()),
        }
    }
}

fn scalar_width(r: Repr) -> Option<usize> {
    match r {
        Repr::UInt8 => Some(1),
        Repr::UInt16 => Some(2),
        Repr::UInt32 | Repr::Float32 => Some(4),
        Repr::UInt64 | Repr::Float => Some(8),
        Repr::USize | Repr::Object => None,
    }
}

fn put_u32(out: &mut Vec<u8>, v: u32) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn put_str(out: &mut Vec<u8>, s: &str) {
    put_u32(out, s.len() as u32);
    out.extend_from_slice(s.as_bytes());
}

// ---------------------------------------------------------------------------------------------
// Reading and writing
// ---------------------------------------------------------------------------------------------

/// A bounds-checked reader of wire data.
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    pub fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(n).filter(|e| *e <= self.data.len());
        match end {
            Some(end) => {
                let s = &self.data[self.pos..end];
                self.pos = end;
                Ok(s)
            }
            None => err(format!("wire data ends after {} bytes; {n} more expected", self.data.len())),
        }
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        Ok(self.take(1)?[0])
    }

    pub fn u16(&mut self) -> Result<u16, WireError> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }

    pub fn u32(&mut self) -> Result<u32, WireError> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    pub fn u64(&mut self) -> Result<u64, WireError> {
        Ok(u64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_le_bytes(self.take(8)?.try_into().unwrap()))
    }

    pub fn f32(&mut self) -> Result<f32, WireError> {
        Ok(f32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }

    /// A length or count, which must fit the remaining data at `unit` bytes per item.
    fn count(&mut self, unit: usize) -> Result<usize, WireError> {
        let n = self.u32()? as usize;
        if n.saturating_mul(unit.max(1)) > self.data.len() - self.pos {
            return err(format!("a length of {n} exceeds the remaining wire data"));
        }
        Ok(n)
    }

    pub fn string(&mut self) -> Result<String, WireError> {
        let n = self.count(1)?;
        let bytes = self.take(n)?;
        match std::str::from_utf8(bytes) {
            Ok(s) => Ok(s.to_owned()),
            Err(_) => err("a string is not valid UTF-8"),
        }
    }

    /// Reads a type expression.
    pub fn ty(&mut self) -> Result<Type, WireError> {
        let t = self.u8()?;
        Ok(match t {
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
            tag::OPTION => Type::Option(Box::new(self.ty()?)),
            tag::LIST => Type::List(Box::new(self.ty()?)),
            tag::ARRAY => Type::Array(Box::new(self.ty()?)),
            tag::PROD => Type::Prod(Box::new(self.ty()?), Box::new(self.ty()?)),
            tag::EXCEPT => Type::Except { error: Box::new(self.ty()?), value: Box::new(self.ty()?) },
            tag::FUNCTION => {
                let n = self.count(1)?;
                let params = (0..n).map(|_| self.ty()).collect::<Result<_, _>>()?;
                Type::Function { params, result: Box::new(self.ty()?) }
            }
            tag::PARAM => Type::Param(self.u32()?),
            tag::INDUCTIVE => {
                let index = self.u32()?;
                let n = self.count(1)?;
                Type::Inductive { index, args: (0..n).map(|_| self.ty()).collect::<Result<_, _>>()? }
            }
            _ => return err(format!("unknown type tag {t}")),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.pos == self.data.len()
    }

    /// The number of bytes not yet read.
    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    /// Fails unless every byte was read.
    pub fn finish(&self) -> Result<(), WireError> {
        if self.is_empty() {
            Ok(())
        } else {
            err(format!("{} unexpected bytes after the wire data", self.data.len() - self.pos))
        }
    }
}

/// Parses a complete type expression.
pub fn parse_type(bytes: &[u8]) -> Result<Type, WireError> {
    let mut r = Reader::new(bytes);
    let t = r.ty()?;
    r.finish()?;
    Ok(t)
}

fn put_len(out: &mut Vec<u8>, n: usize) {
    let n =
        u32::try_from(n).unwrap_or_else(|_| lean_internal_panic("a value exceeds the wire format's 2^32 length limit"));
    put_u32(out, n);
}

// ---------------------------------------------------------------------------------------------
// Handles
// ---------------------------------------------------------------------------------------------

struct HandleTable {
    next: AtomicU64,
    table: Mutex<HashMap<u64, SendObj>>,
}

fn handles() -> &'static HandleTable {
    static H: OnceLock<HandleTable> = OnceLock::new();
    H.get_or_init(|| HandleTable { next: AtomicU64::new(1), table: Mutex::new(HashMap::new()) })
}

/// A new handle for the object `o` (consumed), which becomes shared between threads.
pub fn handle_new(o: Obj) -> u64 {
    unsafe { lean_mark_mt(o) };
    let h = handles();
    let id = h.next.fetch_add(1, Ordering::Relaxed);
    h.table.lock().unwrap_or_else(|p| p.into_inner()).insert(id, SendObj(o));
    id
}

/// The object of handle `id`, retained for the caller.
pub fn handle_get(id: u64) -> Result<Obj, WireError> {
    let table = handles().table.lock().unwrap_or_else(|p| p.into_inner());
    match table.get(&id) {
        Some(o) => {
            unsafe { lean_inc(o.0) };
            Ok(o.0)
        }
        None => err(format!("{id} is not a live handle")),
    }
}

/// A new handle for the object of handle `id`.
pub fn handle_clone(id: u64) -> Result<u64, WireError> {
    handle_get(id).map(handle_new)
}

/// The object of handle `id`, which is released.
pub fn handle_take(id: u64) -> Result<Obj, WireError> {
    match handles().table.lock().unwrap_or_else(|p| p.into_inner()).remove(&id) {
        Some(o) => Ok(o.0),
        None => err(format!("{id} is not a live handle")),
    }
}

/// Releases handle `id`.
pub fn handle_release(id: u64) -> Result<(), WireError> {
    let o = handles().table.lock().unwrap_or_else(|p| p.into_inner()).remove(&id);
    match o {
        Some(o) => {
            unsafe { lean_dec(o.0) };
            Ok(())
        }
        None => err(format!("{id} is not a live handle")),
    }
}

// ---------------------------------------------------------------------------------------------
// Host functions
// ---------------------------------------------------------------------------------------------

/// A buffer of bytes the runtime owns, passed across the C ABI (`lungo_buffer`).
#[repr(C)]
pub struct Buffer {
    pub data: *mut u8,
    pub len: usize,
    pub capacity: usize,
}

impl Buffer {
    pub const fn empty() -> Buffer {
        Buffer { data: std::ptr::null_mut(), len: 0, capacity: 0 }
    }

    /// Stores `bytes` in the buffer, which must be empty.
    pub fn set(&mut self, bytes: Vec<u8>) {
        if !self.data.is_null() {
            lean_internal_panic("a lungo buffer was filled twice");
        }
        let mut bytes = std::mem::ManuallyDrop::new(bytes);
        self.data = bytes.as_mut_ptr();
        self.len = bytes.len();
        self.capacity = bytes.capacity();
    }

    /// Takes the buffer's bytes, leaving it empty.
    pub fn take(&mut self) -> Vec<u8> {
        if self.data.is_null() {
            return Vec::new();
        }
        let v = unsafe { Vec::from_raw_parts(self.data, self.len, self.capacity) };
        *self = Buffer::empty();
        v
    }
}

/// The host's entry points: `dispatch` runs host function `callback` on the encoded arguments
/// and stores the encoded result in `out` (allocated with `lungo_buffer_alloc`), returning 0, or
/// stores a UTF-8 message and returns non-zero if the function failed. The runtime counts its
/// references to a callback with `retain` (when it decodes a host function into a Lean closure)
/// and `release` (when that closure is freed); a host keeps a callback alive while the runtime
/// references it, and while a call it passed the callback to is running.
#[derive(Clone, Copy)]
pub struct Host {
    pub dispatch: unsafe extern "C" fn(callback: u64, input: *const u8, len: usize, out: *mut Buffer) -> i32,
    pub retain: unsafe extern "C" fn(callback: u64),
    pub release: unsafe extern "C" fn(callback: u64),
}

static HOST: OnceLock<Host> = OnceLock::new();

/// Installs the host's entry points, once per process: the language binding's support library
/// does so when it loads.
pub fn set_host(host: Host) {
    if HOST.set(host).is_err() {
        lean_internal_panic("the lungo host entry points are already installed");
    }
}

fn host() -> Host {
    *HOST.get().unwrap_or_else(|| lean_internal_panic("no lungo host is installed to call host functions"))
}

/// Calls host function `callback` with encoded arguments; the result's bytes, or the host's
/// error message.
pub fn call_host(callback: u64, input: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Buffer::empty();
    let status = unsafe { (host().dispatch)(callback, input.as_ptr(), input.len(), &mut out) };
    let bytes = out.take();
    if status == 0 { Ok(bytes) } else { Err(String::from_utf8_lossy(&bytes).into_owned()) }
}

/// The context of a closure over a host function: the callback, the function's type, the
/// program's type table.
struct HostClosure {
    callback: u64,
    params: Vec<Type>,
    result: Type,
    table: &'static TypeTable,
}

unsafe fn host_closure_finalize(data: *mut ()) {
    let c = unsafe { Box::from_raw(data as *mut HostClosure) };
    unsafe { (host().release)(c.callback) };
}

unsafe fn host_closure_for_each(_: *mut (), _: &mut dyn FnMut(Obj)) {}

static HOST_CLOSURE_CLASS: ExternalClass =
    ExternalClass { finalize: host_closure_finalize, for_each: host_closure_for_each };

/// Calls the host function of a host closure (context `ctx`, borrowed) with `args` (owned).
unsafe fn call_host_closure(ctx: Obj, args: &[Obj]) -> Obj {
    let c = unsafe { &*(lean_get_external_data(ctx) as *const HostClosure) };
    let mut input = Vec::new();
    for (a, t) in args.iter().zip(&c.params) {
        unsafe {
            encode(c.table, t, *a, &mut input);
            lean_dec(*a);
        }
    }
    let bytes = call_host(c.callback, &input)
        .unwrap_or_else(|msg| lean_internal_panic(&format!("a host function passed to Lean failed: {msg}")));
    let mut r = Reader::new(&bytes);
    decode(c.table, &c.result, &mut r, Handles::Take).and_then(|v| r.finish().map(|_| v)).unwrap_or_else(|e| {
        lean_internal_panic(&format!("a host function passed to Lean returned malformed data: {e}"))
    })
}

macro_rules! host_trampolines {
    ($($name:ident($($a:ident),+);)*) => {
        $(
            #[allow(clippy::too_many_arguments)]
            unsafe extern "C" fn $name(ctx: Obj, $($a: Obj),+) -> Obj {
                unsafe {
                    let r = call_host_closure(ctx, &[$($a),+]);
                    lean_dec(ctx);
                    r
                }
            }
        )*
    };
}

host_trampolines! {
    host1(a1);
    host2(a1, a2);
    host3(a1, a2, a3);
    host4(a1, a2, a3, a4);
    host5(a1, a2, a3, a4, a5);
    host6(a1, a2, a3, a4, a5, a6);
    host7(a1, a2, a3, a4, a5, a6, a7);
    host8(a1, a2, a3, a4, a5, a6, a7, a8);
    host9(a1, a2, a3, a4, a5, a6, a7, a8, a9);
    host10(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10);
    host11(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11);
    host12(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12);
    host13(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13);
    host14(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14);
    host15(a1, a2, a3, a4, a5, a6, a7, a8, a9, a10, a11, a12, a13, a14, a15);
}

/// A Lean closure calling host function `callback` of type `params → result`.
/// A host function in the arguments of a call is retained; one in a result comes with the
/// reference the host gives up.
fn host_closure(table: &'static TypeTable, callback: u64, params: &[Type], result: &Type, handles: Handles) -> Obj {
    let fun: *const () = match params.len() {
        1 => host1 as *const (),
        2 => host2 as *const (),
        3 => host3 as *const (),
        4 => host4 as *const (),
        5 => host5 as *const (),
        6 => host6 as *const (),
        7 => host7 as *const (),
        8 => host8 as *const (),
        9 => host9 as *const (),
        10 => host10 as *const (),
        11 => host11 as *const (),
        12 => host12 as *const (),
        13 => host13 as *const (),
        14 => host14 as *const (),
        15 => host15 as *const (),
        n => lean_internal_panic(&format!("a host function with {n} parameters")),
    };
    if handles == Handles::Borrow {
        unsafe { (host().retain)(callback) };
    }
    let ctx = Box::new(HostClosure { callback, params: params.to_vec(), result: result.clone(), table });
    unsafe {
        let ext = lean_alloc_external(&HOST_CLOSURE_CLASS, Box::into_raw(ctx) as *mut ());
        let c = lean_alloc_closure(fun, params.len() as u32 + 1, 1);
        lean_closure_set(c, 0, ext);
        c
    }
}

// ---------------------------------------------------------------------------------------------
// Decoding: wire data to Lean objects
// ---------------------------------------------------------------------------------------------

/// How decoding treats the handles in wire data: the arguments of a call lend theirs (the
/// sender keeps them), the result of a call transfers its handles to the receiver. Host
/// functions likewise: one in the arguments is retained, one in a result comes with a
/// reference the host gives up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handles {
    Borrow,
    Take,
}

impl Handles {
    /// The object of handle `id`, owned by the caller.
    fn object(self, id: u64) -> Result<Obj, WireError> {
        match self {
            Handles::Borrow => handle_get(id),
            Handles::Take => handle_take(id),
        }
    }
}

/// Decodes a value of `ty` (without parameters) into an owned object in boxed representation.
pub fn decode(table: &'static TypeTable, ty: &Type, r: &mut Reader, handles: Handles) -> Result<Obj, WireError> {
    unsafe {
        Ok(match ty {
            Type::Nat => {
                let mag = magnitude(r)?;
                crate::nat::nat_from_biguint(mag)
            }
            Type::Int => {
                let sign = r.u8()?;
                let mag = magnitude(r)?;
                let v = match (sign, mag.bits()) {
                    (0, _) => BigInt::from_biguint(Sign::Plus, mag),
                    (1, 0) => return err("negative zero is not a canonical Int"),
                    (1, _) => BigInt::from_biguint(Sign::Minus, mag),
                    (s, _) => return err(format!("invalid Int sign {s}")),
                };
                crate::int::int_from_bigint(v)
            }
            Type::Bool => match r.u8()? {
                b @ (0 | 1) => lean_box(b as usize),
                b => return err(format!("invalid Bool {b}")),
            },
            Type::UInt8 | Type::Int8 => lean_box(r.u8()? as usize),
            Type::UInt16 | Type::Int16 => lean_box(r.u16()? as usize),
            Type::UInt32 | Type::Int32 => lean_box_uint32(r.u32()?),
            Type::Char => {
                let c = r.u32()?;
                if char::from_u32(c).is_none() {
                    return err(format!("{c:#x} is not a Unicode scalar value"));
                }
                lean_box_uint32(c)
            }
            Type::UInt64 | Type::Int64 => lean_box_uint64(r.u64()?),
            Type::USize | Type::ISize => {
                let v = r.u64()?;
                let v = usize::try_from(v).map_err(|_| WireError(format!("{v} does not fit this platform's USize")))?;
                lean_box_usize(v)
            }
            Type::Float => lean_box_float(r.f64()?),
            Type::Float32 => lean_box_float32(r.f32()?),
            Type::String => {
                let n = r.count(1)?;
                let bytes = r.take(n)?;
                let s = std::str::from_utf8(bytes).map_err(|_| WireError("a String is not valid UTF-8".into()))?;
                lean_mk_string_unchecked(bytes, s.chars().count())
            }
            Type::Unit => lean_box(0),
            Type::ByteArray => {
                let n = r.count(1)?;
                let bytes = r.take(n)?;
                let a = lean_alloc_sarray(1, n, n);
                std::ptr::copy_nonoverlapping(bytes.as_ptr(), lean_sarray_cptr(a), n);
                a
            }
            Type::FloatArray => {
                let n = r.count(8)?;
                let a = lean_alloc_sarray(8, n, n);
                let data = lean_sarray_cptr(a) as *mut f64;
                for i in 0..n {
                    data.add(i).write_unaligned(r.f64()?);
                }
                a
            }
            Type::Option(t) => match r.u8()? {
                0 => lean_box(0),
                1 => ctor1(1, decode(table, t, r, handles)?),
                b => return err(format!("invalid Option tag {b}")),
            },
            Type::List(t) => {
                let n = r.count(1)?;
                let mut items = Vec::with_capacity(n);
                for _ in 0..n {
                    match decode(table, t, r, handles) {
                        Ok(v) => items.push(v),
                        Err(e) => {
                            items.into_iter().for_each(|v| lean_dec(v));
                            return Err(e);
                        }
                    }
                }
                let mut out = lean_box(0);
                for v in items.into_iter().rev() {
                    let cell = lean_alloc_ctor(1, 2, 0);
                    lean_ctor_set(cell, 0, v);
                    lean_ctor_set(cell, 1, out);
                    out = cell;
                }
                out
            }
            Type::Array(t) => {
                let n = r.count(1)?;
                let a = lean_alloc_array(0, n);
                for i in 0..n {
                    match decode(table, t, r, handles) {
                        Ok(v) => {
                            lean_array_set_core(a, i, v);
                            lean_array_set_size(a, i + 1);
                        }
                        Err(e) => {
                            lean_dec(a);
                            return Err(e);
                        }
                    }
                }
                a
            }
            Type::Prod(a, b) => {
                let x = decode(table, a, r, handles)?;
                let y = match decode(table, b, r, handles) {
                    Ok(y) => y,
                    Err(e) => {
                        lean_dec(x);
                        return Err(e);
                    }
                };
                let o = lean_alloc_ctor(0, 2, 0);
                lean_ctor_set(o, 0, x);
                lean_ctor_set(o, 1, y);
                o
            }
            Type::Except { error, value } => match r.u8()? {
                0 => ctor1(0, decode(table, error, r, handles)?),
                1 => ctor1(1, decode(table, value, r, handles)?),
                b => return err(format!("invalid Except tag {b}")),
            },
            Type::Function { params, result } => match r.u8()? {
                function::LEAN => handles.object(r.u64()?)?,
                function::HOST => host_closure(table, r.u64()?, params, result, handles),
                b => return err(format!("invalid function kind {b}")),
            },
            Type::Opaque => handles.object(r.u64()?)?,
            Type::Param(i) => return err(format!("an uninstantiated type parameter {i}")),
            Type::Inductive { index, args } => decode_inductive(table, *index, args, r, handles)?,
        })
    }
}

unsafe fn ctor1(tag: u32, v: Obj) -> Obj {
    unsafe {
        let o = lean_alloc_ctor(tag, 1, 0);
        lean_ctor_set(o, 0, v);
        o
    }
}

fn magnitude(r: &mut Reader) -> Result<BigUint, WireError> {
    let n = r.count(1)?;
    let bytes = r.take(n)?;
    if bytes.last() == Some(&0) {
        return err("a number's magnitude has a leading zero byte");
    }
    Ok(BigUint::from_bytes_le(bytes))
}

fn decl(table: &TypeTable, index: u32) -> Result<&TypeDecl, WireError> {
    table.types.get(index as usize).ok_or_else(|| WireError(format!("type index {index} outside the type table")))
}

/// The byte offset of scalar field data at `offset` in constructor `c`.
fn scalar_offset(c: &Ctor, offset: u32) -> usize {
    (c.size + c.usize) as usize * size_of::<usize>() + offset as usize
}

unsafe fn decode_inductive(
    table: &'static TypeTable,
    index: u32,
    args: &[Type],
    r: &mut Reader,
    handles: Handles,
) -> Result<Obj, WireError> {
    let t = decl(table, index)?;
    if args.len() != t.params as usize {
        return err(format!("{} applied to {} arguments", t.name, args.len()));
    }
    if t.opaque {
        return handles.object(r.u64()?);
    }
    if let Some((c, f)) = t.trivial {
        let field = &t.ctors[c as usize].fields[f as usize];
        return decode(table, &field.ty.substitute(args)?, r, handles);
    }
    let i = r.u32()? as usize;
    let c = t.ctors.get(i).ok_or_else(|| WireError(format!("constructor index {i} of {}", t.name)))?;
    unsafe {
        if c.size == 0 && c.usize == 0 && c.ssize == 0 {
            return Ok(lean_box(c.tag as usize));
        }
        let o = lean_alloc_ctor(c.tag, c.size, c.usize as usize * size_of::<usize>() + c.ssize as usize);
        // Object fields are released with the object if decoding fails midway.
        for k in 0..c.size {
            lean_ctor_set(o, k, lean_box(0));
        }
        for f in &c.fields {
            let v = match decode(table, &f.ty.substitute(args)?, r, handles) {
                Ok(v) => v,
                Err(e) => {
                    lean_dec(o);
                    return Err(e);
                }
            };
            match f.kind {
                FieldKind::Object(k) => lean_ctor_set(o, k, v),
                FieldKind::USize(k) => {
                    lean_ctor_set_usize(o, k, lean_unbox_usize(v));
                    lean_dec(v);
                }
                FieldKind::Scalar { offset, repr } => {
                    let at = scalar_offset(c, offset);
                    match repr {
                        Repr::UInt8 => lean_ctor_set_uint8(o, at, lean_unbox(v) as u8),
                        Repr::UInt16 => lean_ctor_set_uint16(o, at, lean_unbox(v) as u16),
                        Repr::UInt32 => lean_ctor_set_uint32(o, at, lean_unbox_uint32(v)),
                        Repr::UInt64 => lean_ctor_set_uint64(o, at, lean_unbox_uint64(v)),
                        Repr::Float => lean_ctor_set_float(o, at, lean_unbox_float(v)),
                        Repr::Float32 => lean_ctor_set_float32(o, at, lean_unbox_float32(v)),
                        Repr::USize | Repr::Object => {
                            lean_internal_panic("a scalar field of non-scalar representation")
                        }
                    }
                    lean_dec(v);
                }
            }
        }
        Ok(o)
    }
}

// ---------------------------------------------------------------------------------------------
// Encoding: Lean objects to wire data
// ---------------------------------------------------------------------------------------------

/// Encodes the object `o` (borrowed, boxed representation) as a value of `ty` (without
/// parameters). Objects that are not values of `ty` violate the program's typing and terminate
/// the process.
///
/// # Safety
///
/// `o` must be a live value of `ty`.
pub unsafe fn encode(table: &'static TypeTable, ty: &Type, o: Obj, out: &mut Vec<u8>) {
    unsafe {
        match ty {
            Type::Nat => {
                let v = crate::nat::nat_to_bigint(o);
                put_magnitude(out, v.magnitude());
            }
            Type::Int => {
                let v = crate::int::int_to_bigint(o);
                out.push(if v.sign() == Sign::Minus { 1 } else { 0 });
                put_magnitude(out, v.magnitude());
            }
            Type::Bool => out.push(lean_unbox(o) as u8),
            Type::UInt8 | Type::Int8 => out.push(lean_unbox(o) as u8),
            Type::UInt16 | Type::Int16 => out.extend_from_slice(&(lean_unbox(o) as u16).to_le_bytes()),
            Type::UInt32 | Type::Int32 | Type::Char => out.extend_from_slice(&lean_unbox_uint32(o).to_le_bytes()),
            Type::UInt64 | Type::Int64 => out.extend_from_slice(&lean_unbox_uint64(o).to_le_bytes()),
            Type::USize | Type::ISize => out.extend_from_slice(&(lean_unbox_usize(o) as u64).to_le_bytes()),
            Type::Float => out.extend_from_slice(&lean_unbox_float(o).to_le_bytes()),
            Type::Float32 => out.extend_from_slice(&lean_unbox_float32(o).to_le_bytes()),
            Type::String => {
                let bytes = lean_string_bytes(o);
                put_len(out, bytes.len());
                out.extend_from_slice(bytes);
            }
            Type::Unit => {}
            Type::ByteArray => {
                let n = lean_sarray_size(o);
                put_len(out, n);
                out.extend_from_slice(std::slice::from_raw_parts(lean_sarray_cptr(o), n));
            }
            Type::FloatArray => {
                let n = lean_sarray_size(o);
                put_len(out, n);
                let data = lean_sarray_cptr(o) as *const f64;
                for i in 0..n {
                    out.extend_from_slice(&data.add(i).read_unaligned().to_le_bytes());
                }
            }
            Type::Option(t) => {
                if o.is_scalar() {
                    out.push(0);
                } else {
                    out.push(1);
                    encode(table, t, lean_ctor_get(o, 0), out);
                }
            }
            Type::List(t) => {
                let mut n = 0usize;
                let mut cur = o;
                while !cur.is_scalar() {
                    n += 1;
                    cur = lean_ctor_get(cur, 1);
                }
                put_len(out, n);
                let mut cur = o;
                while !cur.is_scalar() {
                    encode(table, t, lean_ctor_get(cur, 0), out);
                    cur = lean_ctor_get(cur, 1);
                }
            }
            Type::Array(t) => {
                let n = lean_array_size(o);
                put_len(out, n);
                let data = lean_array_cptr(o);
                for i in 0..n {
                    encode(table, t, *data.add(i), out);
                }
            }
            Type::Prod(a, b) => {
                encode(table, a, lean_ctor_get(o, 0), out);
                encode(table, b, lean_ctor_get(o, 1), out);
            }
            Type::Except { error, value } => {
                let (tag, t) = match lean_ptr_tag(o) {
                    0 => (0u8, error),
                    1 => (1u8, value),
                    k => lean_internal_panic(&format!("invalid Except constructor tag {k}")),
                };
                out.push(tag);
                encode(table, t, lean_ctor_get(o, 0), out);
            }
            Type::Function { .. } => {
                out.push(function::LEAN);
                lean_inc(o);
                out.extend_from_slice(&handle_new(o).to_le_bytes());
            }
            Type::Opaque => {
                lean_inc(o);
                out.extend_from_slice(&handle_new(o).to_le_bytes());
            }
            Type::Param(i) => lean_internal_panic(&format!("encoding an uninstantiated type parameter {i}")),
            Type::Inductive { index, args } => encode_inductive(table, *index, args, o, out),
        }
    }
}

fn put_magnitude(out: &mut Vec<u8>, m: &BigUint) {
    let bytes = if m.bits() == 0 { Vec::new() } else { m.to_bytes_le() };
    put_len(out, bytes.len());
    out.extend_from_slice(&bytes);
}

unsafe fn encode_inductive(table: &'static TypeTable, index: u32, args: &[Type], o: Obj, out: &mut Vec<u8>) {
    let t = decl(table, index).unwrap_or_else(|e| lean_internal_panic(&e.0));
    let sub = |ty: &Type| ty.substitute(args).unwrap_or_else(|e| lean_internal_panic(&e.0));
    unsafe {
        if t.opaque {
            lean_inc(o);
            out.extend_from_slice(&handle_new(o).to_le_bytes());
            return;
        }
        if let Some((c, f)) = t.trivial {
            encode(table, &sub(&t.ctors[c as usize].fields[f as usize].ty), o, out);
            return;
        }
        let tag = lean_obj_tag(o);
        let (i, c) = t
            .ctors
            .iter()
            .enumerate()
            .find(|(_, c)| c.tag == tag)
            .unwrap_or_else(|| lean_internal_panic(&format!("invalid constructor tag {tag} for {}", t.name)));
        put_u32(out, i as u32);
        for f in &c.fields {
            let ty = sub(&f.ty);
            match f.kind {
                FieldKind::Object(k) => encode(table, &ty, lean_ctor_get(o, k), out),
                FieldKind::USize(k) => {
                    let v = lean_box_usize(lean_ctor_get_usize(o, k));
                    encode(table, &ty, v, out);
                    lean_dec(v);
                }
                FieldKind::Scalar { offset, repr } => {
                    let at = scalar_offset(c, offset);
                    let v = match repr {
                        Repr::UInt8 => lean_box(lean_ctor_get_uint8(o, at) as usize),
                        Repr::UInt16 => lean_box(lean_ctor_get_uint16(o, at) as usize),
                        Repr::UInt32 => lean_box_uint32(lean_ctor_get_uint32(o, at)),
                        Repr::UInt64 => lean_box_uint64(lean_ctor_get_uint64(o, at)),
                        Repr::Float => lean_box_float(lean_ctor_get_float(o, at)),
                        Repr::Float32 => lean_box_float32(lean_ctor_get_float32(o, at)),
                        Repr::USize | Repr::Object => {
                            lean_internal_panic("a scalar field of non-scalar representation")
                        }
                    };
                    encode(table, &ty, v, out);
                    lean_dec(v);
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// IO errors
// ---------------------------------------------------------------------------------------------

/// Encodes the `IO.Error` `e` (borrowed): a handle to it and its message, as
/// `IO.Error.toString` renders it.
///
/// # Safety
///
/// `e` must be a live `IO.Error`.
pub unsafe fn encode_io_error(e: Obj, out: &mut Vec<u8>) {
    unsafe {
        let message = crate::init::io_error_to_string(e);
        lean_inc(e);
        out.extend_from_slice(&handle_new(e).to_le_bytes());
        put_len(out, message.len());
        out.extend_from_slice(message.as_bytes());
    }
}

/// Decodes an `IO.Error` a host raised: the one of a handle (transferred to the caller), or,
/// for handle 0, `IO.userError` with the message.
pub fn decode_io_error(r: &mut Reader) -> Result<Obj, WireError> {
    let handle = r.u64()?;
    let message = r.string()?;
    if handle != 0 {
        return handle_take(handle);
    }
    unsafe { Ok(crate::io::mk::user_error(lean_mk_string(&message))) }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> &'static TypeTable {
        // `structure P where x : UInt8, n : Nat`, `inductive T | leaf | node (l : T) (v : Nat) (r : T)`
        // and an opaque `W` (a structure with a proof field, say).
        static T: OnceLock<TypeTable> = OnceLock::new();
        T.get_or_init(|| TypeTable {
            types: vec![
                TypeDecl {
                    name: "P".into(),
                    opaque: false,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![Ctor {
                        name: "P.mk".into(),
                        tag: 0,
                        size: 1,
                        usize: 0,
                        ssize: 1,
                        fields: vec![
                            Field {
                                name: "x".into(),
                                kind: FieldKind::Scalar { offset: 0, repr: Repr::UInt8 },
                                ty: Type::UInt8,
                            },
                            Field { name: "n".into(), kind: FieldKind::Object(0), ty: Type::Nat },
                        ],
                    }],
                },
                TypeDecl {
                    name: "T".into(),
                    opaque: false,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![
                        Ctor { name: "T.leaf".into(), tag: 0, size: 0, usize: 0, ssize: 0, fields: vec![] },
                        Ctor {
                            name: "T.node".into(),
                            tag: 1,
                            size: 3,
                            usize: 0,
                            ssize: 0,
                            fields: vec![
                                Field {
                                    name: "l".into(),
                                    kind: FieldKind::Object(0),
                                    ty: Type::Inductive { index: 1, args: vec![] },
                                },
                                Field { name: "v".into(), kind: FieldKind::Object(1), ty: Type::Nat },
                                Field {
                                    name: "r".into(),
                                    kind: FieldKind::Object(2),
                                    ty: Type::Inductive { index: 1, args: vec![] },
                                },
                            ],
                        },
                    ],
                },
                TypeDecl {
                    name: "W".into(),
                    opaque: true,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![],
                },
            ],
        })
    }

    /// TEST0235: opaque types cross as handles and are declared without a layout
    #[test]
    fn test0235_opaque_types_cross_as_handles_and_are_declared_without_a_layout() {
        let w = Type::Inductive { index: 2, args: vec![] };
        let o = lean_mk_string("a proof-carrying value");
        let mut out = Vec::new();
        unsafe { encode(table(), &w, o, &mut out) };
        assert_eq!(out.len(), 8, "a value of an opaque type is its handle");
        let id = u64::from_le_bytes(out[..8].try_into().unwrap());
        let mut r = Reader::new(&out);
        let back = decode(table(), &w, &mut r, Handles::Take).unwrap();
        r.finish().unwrap();
        assert_eq!(back, o, "the handle names the object that was encoded");
        assert!(handle_get(id).is_err(), "a result's handle is taken");
        unsafe {
            lean_dec(back);
            lean_dec(o);
        }

        // The flag survives the table's encoding.
        let decoded = TypeTable::decode(&table().encode()).unwrap();
        assert_eq!(&decoded, table());

        // An opaque type has no layout to describe: constructors or parameters are refused.
        let mut with_ctor = table().clone();
        with_ctor.types[2].ctors = table().types[0].ctors.clone();
        assert!(TypeTable::decode(&with_ctor.encode()).is_err(), "an opaque type with constructors");
        let mut with_params = table().clone();
        with_params.types[2].params = 1;
        assert!(TypeTable::decode(&with_params.encode()).is_err(), "an opaque type with parameters");
        // And a transparent type still needs its constructors.
        let mut empty = table().clone();
        empty.types[0].ctors.clear();
        assert!(TypeTable::decode(&empty.encode()).is_err(), "a transparent type without constructors");
    }

    fn round_trip(ty: &Type, bytes: &[u8]) {
        let mut r = Reader::new(bytes);
        let o = decode(table(), ty, &mut r, Handles::Borrow).unwrap();
        r.finish().unwrap();
        let mut out = Vec::new();
        unsafe {
            encode(table(), ty, o, &mut out);
            lean_dec(o);
        }
        assert_eq!(out, bytes, "{ty:?}");
    }

    /// TEST0236: values round trip through lean objects
    #[test]
    fn test0236_values_round_trip_through_lean_objects() {
        round_trip(&Type::Nat, &[0, 0, 0, 0]);
        // 2^64: beyond the scalar range of `Nat`.
        round_trip(&Type::Nat, &[9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1]);
        round_trip(&Type::Int, &[1, 1, 0, 0, 0, 5]);
        round_trip(&Type::String, &[3, 0, 0, 0, b'a', 0xc3, 0xa9]);
        round_trip(&Type::List(Box::new(Type::Bool)), &[2, 0, 0, 0, 1, 0]);
        round_trip(&Type::Inductive { index: 0, args: vec![] }, &[0, 0, 0, 0, 7, 1, 0, 0, 0, 3]);
        let node = [1u8, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 2, 0, 0, 0, 0];
        round_trip(&Type::Inductive { index: 1, args: vec![] }, &node);
        round_trip(&Type::Except { error: Box::new(Type::String), value: Box::new(Type::Unit) }, &[1]);
    }

    /// TEST0237: malformed data is rejected not misread
    #[test]
    fn test0237_malformed_data_is_rejected_not_misread() {
        let reject = |ty: Type, bytes: &[u8]| {
            let mut r = Reader::new(bytes);
            if let Ok(o) = decode(table(), &ty, &mut r, Handles::Borrow) {
                unsafe { lean_dec(o) };
                assert!(r.finish().is_err(), "{ty:?} accepted {bytes:?}");
            }
        };
        reject(Type::Bool, &[2]);
        reject(Type::Char, &[0x00, 0xd8, 0, 0]);
        reject(Type::String, &[1, 0, 0, 0, 0xff]);
        reject(Type::String, &[9, 0, 0, 0, b'a']);
        reject(Type::Nat, &[1, 0, 0, 0, 0]);
        reject(Type::Int, &[1, 0, 0, 0, 0]);
        reject(Type::Inductive { index: 1, args: vec![] }, &[2, 0, 0, 0]);
        reject(Type::Opaque, &[99, 0, 0, 0, 0, 0, 0, 0]);
        reject(Type::Unit, &[0]);
    }

    /// TEST0238: handles name live objects until released
    #[test]
    fn test0238_handles_name_live_objects_until_released() {
        let o = lean_mk_string("held");
        let mut out = Vec::new();
        unsafe { encode(table(), &Type::Opaque, o, &mut out) };
        let id = u64::from_le_bytes(out[..8].try_into().unwrap());
        let mut r = Reader::new(&out);
        let back = decode(table(), &Type::Opaque, &mut r, Handles::Borrow).unwrap();
        assert_eq!(back, o, "the handle decodes to the object it names");
        unsafe { lean_dec(back) };
        let copy = handle_clone(id).unwrap();
        assert_ne!(copy, id, "a clone is a handle of its own");
        handle_release(id).unwrap();
        assert!(handle_get(id).is_err(), "a released handle names nothing");
        assert!(handle_release(id).is_err(), "a handle cannot be released twice");
        // A result transfers its handles: decoding takes the handle.
        let copy_bytes = copy.to_le_bytes();
        let mut r = Reader::new(&copy_bytes);
        let taken = decode(table(), &Type::Opaque, &mut r, Handles::Take).unwrap();
        assert_eq!(taken, o);
        assert!(handle_get(copy).is_err(), "a taken handle is released");
        unsafe {
            lean_dec(taken);
            lean_dec(o);
        }
    }

    /// TEST0239: signatures round trip and instantiate
    #[test]
    fn test0239_signatures_round_trip_and_instantiate() {
        let sig = Signature {
            type_params: 1,
            params: vec![Type::List(Box::new(Type::Param(0))), Type::Inductive { index: 0, args: vec![] }],
            returns: Returns::Eio { error: Type::String, value: Type::Param(0) },
        };
        assert_eq!(Signature::decode(&sig.encode()).unwrap(), sig);
        sig.check(table()).unwrap();
        let inst = sig.instantiate(&[Type::Nat]).unwrap();
        assert_eq!(inst.params[0], Type::List(Box::new(Type::Nat)));
        assert_eq!(inst.returns, Returns::Eio { error: Type::String, value: Type::Nat });
        assert!(sig.instantiate(&[]).is_err(), "a type argument is missing");
        let mut bytes = sig.encode();
        bytes.push(0);
        assert!(Signature::decode(&bytes).is_err(), "trailing bytes");
        let unbound = Signature { type_params: 0, params: vec![Type::Param(0)], returns: Returns::Value(Type::Unit) };
        assert!(unbound.check(table()).is_err(), "a parameter the signature does not bind");
    }

    /// TEST0240: type tables round trip and are validated
    #[test]
    fn test0240_type_tables_round_trip_and_are_validated() {
        let bytes = table().encode();
        assert_eq!(&TypeTable::decode(&bytes).unwrap(), table());
        let mut bad = table().clone();
        bad.types[0].ctors[0].fields[1].kind = FieldKind::Object(5);
        assert!(TypeTable::decode(&bad.encode()).is_err());
    }
}
