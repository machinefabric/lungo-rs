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
