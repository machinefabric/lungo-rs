//! The wire format's test vectors (`compiler-tests/wire/vectors.json`): the contract every
//! language's support library is tested against.
//!
//! Each valid vector is a type, a value and its exact encoding; each invalid vector is a type and
//! bytes that must be rejected. This test checks that the committed file is exactly what the
//! reference host codec produces (`LUNGO_BLESS=1` regenerates it), and that the runtime's codec of
//! Lean objects agrees: every valid encoding survives a round trip through Lean objects
//! unchanged, and every invalid one is rejected.

use lungo_runtime::object::lean_dec;
use lungo_runtime::wire::value::{self, FunctionRef, Value};
use lungo_runtime::wire::{self, Reader, Type, TypeTable};
use num_bigint::{BigInt, BigUint};
use serde_json::{Value as Json, json};
use std::path::Path;
use std::sync::OnceLock;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
}

/// The JSON form of a value, as the vectors file records it for every language's tests.
fn json_of(v: &Value) -> Json {
    match v {
        Value::Nat(n) => json!(n.to_string()),
        Value::Int(i) => json!(i.to_string()),
        Value::Bool(b) => json!(b),
        Value::UInt8(x) => json!(x),
        Value::UInt16(x) => json!(x),
        Value::UInt32(x) => json!(x),
        Value::UInt64(x) | Value::USize(x) => json!(x.to_string()),
        Value::Int8(x) => json!(x),
        Value::Int16(x) => json!(x),
        Value::Int32(x) => json!(x),
        Value::Int64(x) | Value::ISize(x) => json!(x.to_string()),
        Value::Float(x) => json!({ "bits": format!("{:016x}", x.to_bits()) }),
        Value::Float32(x) => json!({ "bits": format!("{:08x}", x.to_bits()) }),
        Value::Char(c) => json!(c.to_string()),
        Value::String(s) => json!(s),
        Value::Unit => Json::Null,
        Value::ByteArray(b) => json!({ "bytes": hex(b) }),
        Value::FloatArray(xs) => json!(xs.iter().map(|x| format!("{:016x}", x.to_bits())).collect::<Vec<_>>()),
        Value::Option(None) => json!({ "none": null }),
        Value::Option(Some(x)) => json!({ "some": json_of(x) }),
        Value::List(xs) | Value::Array(xs) => Json::Array(xs.iter().map(json_of).collect()),
        Value::Prod(a, b) => json!([json_of(a), json_of(b)]),
        Value::Except(Ok(x)) => json!({ "ok": json_of(x) }),
        Value::Except(Err(e)) => json!({ "error": json_of(e) }),
        Value::Function(FunctionRef::Lean(h)) => json!({ "lean": h.to_string() }),
        Value::Function(FunctionRef::Host(c)) => json!({ "host": c.to_string() }),
        Value::Opaque(h) => json!({ "handle": h.to_string() }),
        Value::Ctor { .. } => unreachable!("the vectors cover builtin and composite types"),
    }
}

fn nat(s: &str) -> Value {
    Value::Nat(s.parse::<BigUint>().unwrap())
}

fn int(s: &str) -> Value {
    Value::Int(s.parse::<BigInt>().unwrap())
}

fn b(t: Type) -> Box<Type> {
    Box::new(t)
}

fn bv(v: Value) -> Box<Value> {
    Box::new(v)
}

/// Every valid vector: a name, a type and a value.
fn valid() -> Vec<(&'static str, Type, Value)> {
    vec![
        ("nat zero", Type::Nat, nat("0")),
        ("nat one byte", Type::Nat, nat("255")),
        ("nat two bytes", Type::Nat, nat("256")),
        ("nat 2^64", Type::Nat, nat("18446744073709551616")),
        ("nat 2^200", Type::Nat, nat("1606938044258990275541962092341162602522202993782792835301376")),
        ("int zero", Type::Int, int("0")),
        ("int minus one", Type::Int, int("-1")),
        ("int 2^63", Type::Int, int("9223372036854775808")),
        ("int -(2^70)", Type::Int, int("-1180591620717411303424")),
        ("bool true", Type::Bool, Value::Bool(true)),
        ("bool false", Type::Bool, Value::Bool(false)),
        ("uint8 max", Type::UInt8, Value::UInt8(u8::MAX)),
        ("uint16 max", Type::UInt16, Value::UInt16(u16::MAX)),
        ("uint32 max", Type::UInt32, Value::UInt32(u32::MAX)),
        ("uint64 max", Type::UInt64, Value::UInt64(u64::MAX)),
        ("usize", Type::USize, Value::USize(1234)),
        ("int8 min", Type::Int8, Value::Int8(i8::MIN)),
        ("int16 minus two", Type::Int16, Value::Int16(-2)),
        ("int32 min", Type::Int32, Value::Int32(i32::MIN)),
        ("int64 min", Type::Int64, Value::Int64(i64::MIN)),
        ("isize minus five", Type::ISize, Value::ISize(-5)),
        ("float", Type::Float, Value::Float(1.5)),
        ("float negative zero", Type::Float, Value::Float(-0.0)),
        ("float infinity", Type::Float, Value::Float(f64::INFINITY)),
        ("float nan payload", Type::Float, Value::Float(f64::from_bits(0x7ff8_0000_0000_0001))),
        ("float32", Type::Float32, Value::Float32(3.25)),
        ("char ascii", Type::Char, Value::Char('a')),
        ("char two bytes", Type::Char, Value::Char('é')),
        ("char astral", Type::Char, Value::Char('😀')),
        ("string empty", Type::String, Value::String(String::new())),
        ("string unicode", Type::String, Value::String("héllo ⅌ 😀".into())),
        ("string embedded nul", Type::String, Value::String("a\0b".into())),
        ("unit", Type::Unit, Value::Unit),
        ("byte array empty", Type::ByteArray, Value::ByteArray(vec![])),
        ("byte array", Type::ByteArray, Value::ByteArray(vec![0, 255, 7])),
        ("float array", Type::FloatArray, Value::FloatArray(vec![1.0, -2.5])),
        ("option none", Type::Option(b(Type::Nat)), Value::Option(None)),
        ("option some", Type::Option(b(Type::Nat)), Value::Option(Some(bv(nat("5"))))),
        ("list empty", Type::List(b(Type::UInt8)), Value::List(vec![])),
        (
            "list",
            Type::List(b(Type::UInt8)),
            Value::List(vec![Value::UInt8(1), Value::UInt8(2), Value::UInt8(3)]),
        ),
        (
            "array of strings",
            Type::Array(b(Type::String)),
            Value::Array(vec![Value::String("x".into()), Value::String("yz".into())]),
        ),
        (
            "prod",
            Type::Prod(b(Type::Bool), b(Type::String)),
            Value::Prod(bv(Value::Bool(true)), bv(Value::String("ok".into()))),
        ),
        (
            "except error",
            Type::Except { error: b(Type::String), value: b(Type::Nat) },
            Value::Except(Err(bv(Value::String("bad".into())))),
        ),
        (
            "except ok",
            Type::Except { error: b(Type::String), value: b(Type::Nat) },
            Value::Except(Ok(bv(nat("42")))),
        ),
        (
            "nested",
            Type::Option(b(Type::List(b(Type::Int)))),
            Value::Option(Some(bv(Value::List(vec![int("-3"), int("0"), int("99999999999999999999")])))),
        ),
        (
            "function lean",
            Type::Function { params: vec![Type::Nat], result: b(Type::Nat) },
            Value::Function(FunctionRef::Lean(7)),
        ),
        (
            "function host",
            Type::Function { params: vec![Type::Nat, Type::Bool], result: b(Type::String) },
            Value::Function(FunctionRef::Host(9)),
        ),
        ("opaque", Type::Opaque, Value::Opaque(42)),
    ]
}

/// Every invalid vector: a name, a type and bytes both codecs must reject.
fn invalid() -> Vec<(&'static str, Type, Vec<u8>)> {
    vec![
        ("bool out of range", Type::Bool, vec![2]),
        ("char surrogate", Type::Char, 0xd800u32.to_le_bytes().to_vec()),
        ("char beyond unicode", Type::Char, 0x110000u32.to_le_bytes().to_vec()),
        ("string invalid utf-8", Type::String, vec![1, 0, 0, 0, 0xff]),
        ("string longer than data", Type::String, vec![9, 0, 0, 0, b'a']),
        ("nat leading zero byte", Type::Nat, vec![1, 0, 0, 0, 0]),
        ("int negative zero", Type::Int, vec![1, 0, 0, 0, 0]),
        ("int invalid sign", Type::Int, vec![2, 1, 0, 0, 0, 1]),
        ("option invalid tag", Type::Option(b(Type::Nat)), vec![2]),
        ("except invalid tag", Type::Except { error: b(Type::Nat), value: b(Type::Nat) }, vec![2, 0, 0, 0, 0]),
        ("function invalid kind", Type::Function { params: vec![Type::Nat], result: b(Type::Nat) }, vec![2, 0, 0, 0, 0, 0, 0, 0, 0]),
        ("trailing bytes", Type::Unit, vec![0]),
        ("truncated", Type::UInt32, vec![1, 2, 3]),
        ("list longer than data", Type::List(b(Type::UInt8)), vec![5, 0, 0, 0, 1]),
    ]
}

fn table() -> &'static TypeTable {
    static T: OnceLock<TypeTable> = OnceLock::new();
    T.get_or_init(TypeTable::default)
}

fn encoded_type(t: &Type) -> Vec<u8> {
    let mut out = Vec::new();
    t.encode(&mut out);
    out
}

fn document() -> Json {
    let valid: Vec<Json> = valid()
        .iter()
        .map(|(name, ty, v)| {
            let mut bytes = Vec::new();
            value::encode(table(), ty, v, &mut bytes).unwrap_or_else(|e| panic!("{name}: {e}"));
            json!({ "name": name, "type": hex(&encoded_type(ty)), "value": json_of(v), "bytes": hex(&bytes) })
        })
        .collect();
    let invalid: Vec<Json> = invalid()
        .iter()
        .map(|(name, ty, bytes)| json!({ "name": name, "type": hex(&encoded_type(ty)), "bytes": hex(bytes) }))
        .collect();
    json!({
        "description": "Test vectors of the lungo wire format: every language's support library must encode each valid value to exactly its bytes, decode the bytes to the value, and reject every invalid encoding. Types are encoded type expressions; see the wire-format reference for the JSON conventions of values.",
        "version": wire::TABLE_VERSION,
        "valid": valid,
        "invalid": invalid
    })
}

fn path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../compiler-tests/wire/vectors.json")
}

#[test]
fn vectors_file_is_the_reference_encoding() {
    let mut text = serde_json::to_string_pretty(&document()).unwrap();
    text.push('\n');
    if std::env::var_os("LUNGO_BLESS").is_some() {
        std::fs::create_dir_all(path().parent().unwrap()).unwrap();
        std::fs::write(path(), &text).unwrap();
        return;
    }
    let committed = std::fs::read_to_string(path()).expect("compiler-tests/wire/vectors.json exists");
    assert!(committed == text, "vectors.json is out of date; run `LUNGO_BLESS=1 cargo test -p lungo-runtime --test wire_vectors`");
}

#[test]
fn valid_encodings_round_trip_through_lean_objects() {
    let file: Json = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    for v in file["valid"].as_array().unwrap() {
        let ty = wire::parse_type(&unhex(v["type"].as_str().unwrap())).unwrap();
        // Handles name live objects of a running program; the vectors pin only their encoding
        // (handles are tested against live objects in the runtime's unit tests).
        if matches!(ty, Type::Function { .. } | Type::Opaque) {
            continue;
        }
        let bytes = unhex(v["bytes"].as_str().unwrap());
        let mut r = Reader::new(&bytes);
        let o = wire::decode(table(), &ty, &mut r, wire::Handles::Borrow).unwrap_or_else(|e| panic!("{}: {e}", v["name"]));
        r.finish().unwrap_or_else(|e| panic!("{}: {e}", v["name"]));
        let mut again = Vec::new();
        unsafe {
            wire::encode(table(), &ty, o, &mut again);
            lean_dec(o);
        }
        assert_eq!(again, bytes, "{}", v["name"]);
    }
}

#[test]
fn invalid_encodings_are_rejected_by_both_codecs() {
    let file: Json = serde_json::from_str(&std::fs::read_to_string(path()).unwrap()).unwrap();
    for v in file["invalid"].as_array().unwrap() {
        let ty = wire::parse_type(&unhex(v["type"].as_str().unwrap())).unwrap();
        let bytes = unhex(v["bytes"].as_str().unwrap());
        let mut r = Reader::new(&bytes);
        let host = value::decode(table(), &ty, &mut r).and_then(|_| r.finish());
        assert!(host.is_err(), "the reference codec accepts {}", v["name"]);
        let mut r = Reader::new(&bytes);
        match wire::decode(table(), &ty, &mut r, wire::Handles::Borrow) {
            Ok(o) => {
                unsafe { lean_dec(o) };
                assert!(r.finish().is_err(), "the Lean codec accepts {}", v["name"]);
            }
            Err(_) => {}
        }
    }
}
