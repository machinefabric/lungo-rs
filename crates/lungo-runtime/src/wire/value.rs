//! Host-side values: Lean values as plain data, encoded and decoded by their [`Type`] without
//! Lean objects. This is the reference host codec of the wire format; the C binding's value API
//! is built on it, and the support libraries of the other languages implement the same encoding
//! (checked by the shared test vectors).

use super::{Reader, Type, TypeTable, WireError, err, function, put_len, put_u32};
use num_bigint::{BigInt, BigUint, Sign};

/// A function value: a Lean closure by handle, or a host function by callback identifier.
#[derive(Debug, Clone, PartialEq)]
pub enum FunctionRef {
    Lean(u64),
    Host(u64),
}

/// A value of a type expression.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Nat(BigUint),
    Int(BigInt),
    Bool(bool),
    UInt8(u8),
    UInt16(u16),
    UInt32(u32),
    UInt64(u64),
    USize(u64),
    Int8(i8),
    Int16(i16),
    Int32(i32),
    Int64(i64),
    ISize(i64),
    Float(f64),
    Float32(f32),
    Char(char),
    String(String),
    Unit,
    ByteArray(Vec<u8>),
    FloatArray(Vec<f64>),
    Option(Option<Box<Value>>),
    List(Vec<Value>),
    Array(Vec<Value>),
    Prod(Box<Value>, Box<Value>),
    /// `Except.error` (`Err`) or `Except.ok` (`Ok`).
    Except(Result<Box<Value>, Box<Value>>),
    Function(FunctionRef),
    /// An opaque Lean value, by handle.
    Opaque(u64),
    /// A value of an inductive type: the constructor's index in the type table and its fields,
    /// in order. A single-constructor type represented by one field is encoded as that field.
    Ctor {
        index: u32,
        fields: Vec<Value>,
    },
}

fn mismatch<T>(ty: &Type, v: &Value) -> Result<T, WireError> {
    err(format!("a value {v:?} is not of type {ty:?}"))
}

/// Encodes `v` as a value of `ty` (without parameters).
pub fn encode(table: &TypeTable, ty: &Type, v: &Value, out: &mut Vec<u8>) -> Result<(), WireError> {
    match (ty, v) {
        (Type::Nat, Value::Nat(n)) => put_magnitude(out, n),
        (Type::Int, Value::Int(i)) => {
            out.push(if i.sign() == Sign::Minus { 1 } else { 0 });
            put_magnitude(out, i.magnitude());
        }
        (Type::Bool, Value::Bool(b)) => out.push(*b as u8),
        (Type::UInt8, Value::UInt8(x)) => out.push(*x),
        (Type::UInt16, Value::UInt16(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::UInt32, Value::UInt32(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::UInt64, Value::UInt64(x)) | (Type::USize, Value::USize(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Int8, Value::Int8(x)) => out.push(*x as u8),
        (Type::Int16, Value::Int16(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Int32, Value::Int32(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Int64, Value::Int64(x)) | (Type::ISize, Value::ISize(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Float, Value::Float(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Float32, Value::Float32(x)) => out.extend_from_slice(&x.to_le_bytes()),
        (Type::Char, Value::Char(c)) => out.extend_from_slice(&(*c as u32).to_le_bytes()),
        (Type::String, Value::String(s)) => {
            put_len(out, s.len());
            out.extend_from_slice(s.as_bytes());
        }
        (Type::Unit, Value::Unit) => {}
        (Type::ByteArray, Value::ByteArray(b)) => {
            put_len(out, b.len());
            out.extend_from_slice(b);
        }
        (Type::FloatArray, Value::FloatArray(xs)) => {
            put_len(out, xs.len());
            for x in xs {
                out.extend_from_slice(&x.to_le_bytes());
            }
        }
        (Type::Option(t), Value::Option(o)) => match o {
            None => out.push(0),
            Some(x) => {
                out.push(1);
                encode(table, t, x, out)?;
            }
        },
        (Type::List(t), Value::List(xs)) | (Type::Array(t), Value::Array(xs)) => {
            put_len(out, xs.len());
            for x in xs {
                encode(table, t, x, out)?;
            }
        }
        (Type::Prod(a, b), Value::Prod(x, y)) => {
            encode(table, a, x, out)?;
            encode(table, b, y, out)?;
        }
        (Type::Except { error, value }, Value::Except(r)) => match r {
            Err(e) => {
                out.push(0);
                encode(table, error, e, out)?;
            }
            Ok(x) => {
                out.push(1);
                encode(table, value, x, out)?;
            }
        },
        (Type::Function { .. }, Value::Function(f)) => match f {
            FunctionRef::Lean(h) => {
                out.push(function::LEAN);
                out.extend_from_slice(&h.to_le_bytes());
            }
            FunctionRef::Host(c) => {
                out.push(function::HOST);
                out.extend_from_slice(&c.to_le_bytes());
            }
        },
        (Type::Opaque, Value::Opaque(h)) => out.extend_from_slice(&h.to_le_bytes()),
        (Type::Inductive { index, .. }, Value::Opaque(h)) if super::decl(table, *index)?.opaque => {
            out.extend_from_slice(&h.to_le_bytes())
        }
        (Type::Inductive { index, args }, Value::Ctor { index: c, fields }) => {
            let decl = super::decl(table, *index)?;
            if decl.opaque {
                return err(format!("{} is opaque: its values are handles, not constructors", decl.name));
            }
            let ctor = decl
                .ctors
                .get(*c as usize)
                .ok_or_else(|| super::WireError(format!("constructor index {c} of {}", decl.name)))?;
            if fields.len() != ctor.fields.len() {
                return err(format!("{} takes {} fields, not {}", ctor.name, ctor.fields.len(), fields.len()));
            }
            match decl.trivial {
                Some((_, f)) => encode(table, &ctor.fields[f as usize].ty.substitute(args)?, &fields[f as usize], out)?,
                None => {
                    put_u32(out, *c);
                    for (field, v) in ctor.fields.iter().zip(fields) {
                        encode(table, &field.ty.substitute(args)?, v, out)?;
                    }
                }
            }
        }
        (Type::Param(i), _) => return err(format!("an uninstantiated type parameter {i}")),
        (ty, v) => return mismatch(ty, v),
    }
    Ok(())
}

fn put_magnitude(out: &mut Vec<u8>, m: &BigUint) {
    let bytes = if m.bits() == 0 { Vec::new() } else { m.to_bytes_le() };
    put_len(out, bytes.len());
    out.extend_from_slice(&bytes);
}

/// Decodes a value of `ty` (without parameters).
pub fn decode(table: &TypeTable, ty: &Type, r: &mut Reader) -> Result<Value, WireError> {
    Ok(match ty {
        Type::Nat => Value::Nat(super::magnitude(r)?),
        Type::Int => {
            let sign = r.u8()?;
            let mag = super::magnitude(r)?;
            match (sign, mag.bits()) {
                (0, _) => Value::Int(BigInt::from_biguint(Sign::Plus, mag)),
                (1, 0) => return err("negative zero is not a canonical Int"),
                (1, _) => Value::Int(BigInt::from_biguint(Sign::Minus, mag)),
                (s, _) => return err(format!("invalid Int sign {s}")),
            }
        }
        Type::Bool => match r.u8()? {
            0 => Value::Bool(false),
            1 => Value::Bool(true),
            b => return err(format!("invalid Bool {b}")),
        },
        Type::UInt8 => Value::UInt8(r.u8()?),
        Type::UInt16 => Value::UInt16(r.u16()?),
        Type::UInt32 => Value::UInt32(r.u32()?),
        Type::UInt64 => Value::UInt64(r.u64()?),
        Type::USize => Value::USize(r.u64()?),
        Type::Int8 => Value::Int8(r.u8()? as i8),
        Type::Int16 => Value::Int16(r.u16()? as i16),
        Type::Int32 => Value::Int32(r.u32()? as i32),
        Type::Int64 => Value::Int64(r.u64()? as i64),
        Type::ISize => Value::ISize(r.u64()? as i64),
        Type::Float => Value::Float(r.f64()?),
        Type::Float32 => Value::Float32(r.f32()?),
        Type::Char => {
            let c = r.u32()?;
            Value::Char(char::from_u32(c).ok_or_else(|| WireError(format!("{c:#x} is not a Unicode scalar value")))?)
        }
        Type::String => Value::String(r.string()?),
        Type::Unit => Value::Unit,
        Type::ByteArray => {
            let n = r.count(1)?;
            Value::ByteArray(r.take(n)?.to_vec())
        }
        Type::FloatArray => {
            let n = r.count(8)?;
            Value::FloatArray((0..n).map(|_| r.f64()).collect::<Result<_, _>>()?)
        }
        Type::Option(t) => match r.u8()? {
            0 => Value::Option(None),
            1 => Value::Option(Some(Box::new(decode(table, t, r)?))),
            b => return err(format!("invalid Option tag {b}")),
        },
        Type::List(t) => {
            let n = r.count(1)?;
            Value::List((0..n).map(|_| decode(table, t, r)).collect::<Result<_, _>>()?)
        }
        Type::Array(t) => {
            let n = r.count(1)?;
            Value::Array((0..n).map(|_| decode(table, t, r)).collect::<Result<_, _>>()?)
        }
        Type::Prod(a, b) => Value::Prod(Box::new(decode(table, a, r)?), Box::new(decode(table, b, r)?)),
        Type::Except { error, value } => match r.u8()? {
            0 => Value::Except(Err(Box::new(decode(table, error, r)?))),
            1 => Value::Except(Ok(Box::new(decode(table, value, r)?))),
            b => return err(format!("invalid Except tag {b}")),
        },
        Type::Function { .. } => match r.u8()? {
            function::LEAN => Value::Function(FunctionRef::Lean(r.u64()?)),
            function::HOST => Value::Function(FunctionRef::Host(r.u64()?)),
            b => return err(format!("invalid function kind {b}")),
        },
        Type::Opaque => Value::Opaque(r.u64()?),
        Type::Param(i) => return err(format!("an uninstantiated type parameter {i}")),
        Type::Inductive { index, args } => {
            let decl = super::decl(table, *index)?;
            if decl.opaque {
                return Ok(Value::Opaque(r.u64()?));
            }
            match decl.trivial {
                Some((c, f)) => {
                    let ctor = &decl.ctors[c as usize];
                    if ctor.fields.len() != 1 || f != 0 {
                        return err(format!("{} is represented by one of several fields", decl.name));
                    }
                    Value::Ctor { index: c, fields: vec![decode(table, &ctor.fields[0].ty.substitute(args)?, r)?] }
                }
                None => {
                    let c = r.u32()?;
                    let ctor = decl
                        .ctors
                        .get(c as usize)
                        .ok_or_else(|| WireError(format!("constructor index {c} of {}", decl.name)))?;
                    let fields = ctor
                        .fields
                        .iter()
                        .map(|f| f.ty.substitute(args).and_then(|t| decode(table, &t, r)))
                        .collect::<Result<_, _>>()?;
                    Value::Ctor { index: c, fields }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::{Ctor, Repr, TypeDecl};

    fn table() -> TypeTable {
        TypeTable {
            types: vec![
                TypeDecl {
                    name: "W".into(),
                    opaque: true,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![],
                },
                TypeDecl {
                    name: "E".into(),
                    opaque: false,
                    params: 0,
                    repr: Repr::Object,
                    trivial: None,
                    ctors: vec![Ctor { name: "E.e".into(), tag: 0, size: 0, usize: 0, ssize: 0, fields: vec![] }],
                },
            ],
        }
    }

    #[test]
    fn a_value_of_an_opaque_type_is_its_handle() {
        let t = table();
        let w = Type::Inductive { index: 0, args: vec![] };
        let mut out = Vec::new();
        encode(&t, &w, &Value::Opaque(42), &mut out).unwrap();
        assert_eq!(out, 42u64.to_le_bytes());
        let mut r = Reader::new(&out);
        assert_eq!(decode(&t, &w, &mut r).unwrap(), Value::Opaque(42));
        r.finish().unwrap();
        // Its values are handles: a constructor is not one of them, and a handle is not a value
        // of a transparent type.
        let ctor = Value::Ctor { index: 0, fields: vec![] };
        assert!(encode(&t, &w, &ctor, &mut Vec::new()).is_err());
        let e = Type::Inductive { index: 1, args: vec![] };
        assert!(encode(&t, &e, &Value::Opaque(42), &mut Vec::new()).is_err());
    }
}
