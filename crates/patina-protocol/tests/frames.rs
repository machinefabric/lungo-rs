//! The worker protocol's framing and the strictness of its payload decoding: operations, fields
//! or representations the Rust side does not know are hard errors, never ignored.

use ciborium::Value;
use patina_bir::*;
use patina_protocol::{FrameError, FrameKind, PROTOCOL_VERSION, decode_frame, encode_frame};

fn sample() -> Program {
    let body = Block {
        stmts: vec![
            Stmt::Let { var: 2, ty: IrType::Object, expr: Expr::Lit(Literal::Str("x".into())) },
            Stmt::SetTag { var: 2, tag: 1 },
        ],
        terminator: Terminator::Ret { arg: Arg::Var(2) },
    };
    Program {
        bir_version: BIR_VERSION,
        modules: vec![Module { name: "M".into(), imports: vec![], initializers: vec![Initializer::Io("f".into())] }],
        declarations: vec![Declaration {
            name: "f".into(),
            module: "M".into(),
            origin: Some("f".into()),
            params: vec![Param { var: 1, ty: IrType::Uint64, borrow: false }],
            result: IrType::Object,
            body: Body::Function { block: body },
        }],
    }
}

/// The CBOR data model of `sample()`, for targeted corruption.
fn sample_value() -> Value {
    Value::serialized(&sample()).unwrap()
}

/// The entries of a CBOR map.
type Entries = Vec<(Value, Value)>;

/// Applies `f` to every map in `v`, recursively.
fn each_map(v: &mut Value, f: &mut dyn FnMut(&mut Entries)) {
    match v {
        Value::Map(entries) => {
            f(entries);
            for (_, x) in entries.iter_mut() {
                each_map(x, f);
            }
        }
        Value::Array(xs) => xs.iter_mut().for_each(|x| each_map(x, f)),
        _ => {}
    }
}

fn rename_key(v: &mut Value, from: &str, to: &str) {
    each_map(v, &mut |entries| {
        for (k, _) in entries.iter_mut() {
            if k.as_text() == Some(from) {
                *k = Value::Text(to.into());
            }
        }
    });
}

fn decode_program(v: &Value) -> Result<Program, FrameError> {
    decode_frame(FrameKind::Response, &encode_frame(FrameKind::Response, v).unwrap())
}

#[test]
fn programs_round_trip() {
    let frame = encode_frame(FrameKind::Response, &sample()).unwrap();
    assert_eq!(&frame[..4], b"PTNF");
    assert_eq!(frame[4], FrameKind::Response as u8);
    assert_eq!(u32::from_le_bytes(frame[5..9].try_into().unwrap()), PROTOCOL_VERSION);
    assert_eq!(u64::from_le_bytes(frame[9..17].try_into().unwrap()) as usize, frame.len() - 17);
    let back: Program = decode_frame(FrameKind::Response, &frame).unwrap();
    assert_eq!(back, sample());
    assert!(decode_program(&sample_value()).is_ok());
}

#[test]
fn unknown_operations_are_rejected() {
    // An unknown statement.
    let mut v = sample_value();
    rename_key(&mut v, "set_tag", "set_colour");
    let err = decode_program(&v).unwrap_err().to_string();
    assert!(err.contains("set_colour"), "{err}");
    // An unknown expression.
    let mut v = sample_value();
    rename_key(&mut v, "lit", "literal_v2");
    assert!(decode_program(&v).unwrap_err().to_string().contains("literal_v2"));
    // An unknown terminator.
    let mut v = sample_value();
    rename_key(&mut v, "ret", "tail_call");
    assert!(decode_program(&v).unwrap_err().to_string().contains("tail_call"));
    // An unknown body kind.
    let mut v = sample_value();
    rename_key(&mut v, "function", "bytecode");
    assert!(decode_program(&v).unwrap_err().to_string().contains("bytecode"));
}

#[test]
fn unknown_representations_are_rejected() {
    let mut v = sample_value();
    each_map(&mut v, &mut |entries| {
        for (k, x) in entries.iter_mut() {
            if k.as_text() == Some("ty") && x.as_text() == Some("uint64") {
                *x = Value::Text("uint128".into());
            }
        }
    });
    assert!(decode_program(&v).unwrap_err().to_string().contains("uint128"));
}

#[test]
fn unknown_fields_are_rejected() {
    let mut v = sample_value();
    each_map(&mut v, &mut |entries| {
        if entries.iter().any(|(k, _)| k.as_text() == Some("origin")) {
            entries.push((Value::Text("inline_hint".into()), Value::Bool(true)));
        }
    });
    assert!(decode_program(&v).unwrap_err().to_string().contains("inline_hint"));
}

#[test]
fn missing_fields_are_rejected() {
    let mut v = sample_value();
    each_map(&mut v, &mut |entries| entries.retain(|(k, _)| k.as_text() != Some("borrow")));
    assert!(decode_program(&v).unwrap_err().to_string().contains("borrow"));
}

#[test]
fn corrupt_frames_are_rejected() {
    let good = encode_frame(FrameKind::Response, &sample()).unwrap();
    let decode = |f: &[u8]| decode_frame::<Program>(FrameKind::Response, f).unwrap_err();

    assert!(matches!(decode(&good[..10]), FrameError::Truncated));
    let mut f = good.clone();
    f[0] = b'X';
    assert!(matches!(decode(&f), FrameError::InvalidMagic));
    assert!(matches!(
        decode_frame::<Program>(FrameKind::Request, &good).unwrap_err(),
        FrameError::UnexpectedKind { expected: FrameKind::Request, found: 2 }
    ));
    let mut f = good.clone();
    f[5..9].copy_from_slice(&(PROTOCOL_VERSION + 1).to_le_bytes());
    assert!(matches!(decode(&f), FrameError::ProtocolVersion(v) if v == PROTOCOL_VERSION + 1));
    assert!(matches!(decode(&good[..good.len() - 1]), FrameError::InvalidLength { .. }));
    let mut f = good.clone();
    f.push(0);
    assert!(matches!(decode(&f), FrameError::InvalidLength { .. }));
    // A header that matches a corrupt payload.
    let mut f = good[..17].to_vec();
    f.extend_from_slice(&[0xff; 8]);
    f[9..17].copy_from_slice(&8u64.to_le_bytes());
    assert!(matches!(decode(&f), FrameError::Decode(_)));
}
