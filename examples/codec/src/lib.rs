//! A varint (LEB128) codec compiled from Lean, proved to round-trip (`Varint.decode_encode`,
//! `Varint.decodeAll_encodeAll`) and to refuse input that ends inside a number
//! (`Varint.decode_truncated`).

lungo::include_lean!("varint");
