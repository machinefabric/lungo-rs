use lean2rust_bir::{DeclarationValue, ExternEntry, FrameError, Module};

const SIMPLE: &[u8] = include_bytes!("../../../compiler-tests/golden/simple.bir");

#[test]
fn decodes_real_lean_worker_output() {
    let module = Module::from_frame(SIMPLE).expect("Lean 4.34.1 worker frame must decode");
    assert_eq!(module.module, "Simple");
    let extern_decl = module
        .declarations
        .iter()
        .find(|declaration| declaration.name == "providerSend")
        .expect("extern must be present in final LCNF");
    let DeclarationValue::Extern { entries } = &extern_decl.value else {
        panic!("providerSend must remain an extern requirement");
    };
    assert!(entries.iter().any(|entry| matches!(
        entry,
        ExternEntry::Standard { symbol, .. } if symbol == "provider_send"
    )));
    assert!(
        module
            .declarations
            .iter()
            .any(|declaration| declaration.name == "twice")
    );
    assert!(
        !module
            .declarations
            .iter()
            .any(|declaration| declaration.name == "twice_zero")
    );
}

#[test]
fn rejects_truncated_and_wrong_length_frames() {
    assert!(matches!(
        Module::from_frame(&SIMPLE[..7]),
        Err(FrameError::Truncated)
    ));
    let mut frame = SIMPLE.to_vec();
    frame[4] = frame[4].wrapping_add(1);
    assert!(matches!(
        Module::from_frame(&frame),
        Err(FrameError::InvalidLength { .. })
    ));
}

fn frame_with_payload(payload: &serde_json::Value) -> Vec<u8> {
    let encoded = serde_json::to_vec(payload).unwrap();
    let mut frame = b"L2RB".to_vec();
    frame.extend_from_slice(&(encoded.len() as u32).to_le_bytes());
    frame.extend_from_slice(&encoded);
    frame
}

#[test]
fn rejects_noncanonical_and_out_of_range_worker_integers() {
    let mut payload: serde_json::Value = serde_json::from_slice(&SIMPLE[8..]).unwrap();
    let decls = payload["declarations"].as_array_mut().unwrap();
    let boxed_index = decls
        .iter()
        .position(|decl| decl["name"] == "twice._boxed")
        .unwrap();
    decls[boxed_index]["value"]["body"]["next"]["count"] = "01".into();
    assert!(matches!(
        Module::from_frame(&frame_with_payload(&payload)),
        Err(FrameError::InvalidPayload(_))
    ));

    let twice_index = decls.iter().position(|decl| decl["name"] == "twice").unwrap();
    decls[twice_index]["value"]["body"]["value"] = serde_json::json!({
        "op": "literal",
        "literal": {"op": "uint8", "value": "256"}
    });
    decls[boxed_index]["value"]["body"]["next"]["count"] = "1".into();
    assert!(matches!(
        Module::from_frame(&frame_with_payload(&payload)),
        Err(FrameError::InvalidPayload(_))
    ));
}
