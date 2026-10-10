use codec::{__meta, decode, decode_all, encode, encode_all};
use lungo::{ClaimStatus, List, Nat};

/// LEB128, written independently of the Lean definition.
fn reference(mut n: u128) -> Vec<u8> {
    let mut out = Vec::new();
    loop {
        let low = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(low);
            return out;
        }
        out.push(low | 0x80);
    }
}

fn bytes(xs: &[u8]) -> List<u8> {
    List::from(xs.to_vec())
}

/// TEST0314: encode agrees with LEB128
#[test]
fn test0314_encode_agrees_with_leb128() {
    assert_eq!(encode(Nat::from(0u64)), bytes(&[0]));
    assert_eq!(encode(Nat::from(300u64)), bytes(&[0xac, 0x02]));
    assert_eq!(encode(Nat::from(624485u64)), bytes(&[0xe5, 0x8e, 0x26]));
    let mut x: u128 = 1;
    while x < u128::MAX / 3 {
        assert_eq!(encode(Nat::from(x)), bytes(&reference(x)), "{x}");
        x = x * 3 + 1;
    }
}

/// TEST0315: decode reads what encode wrote and refuses truncated input
#[test]
fn test0315_decode_reads_what_encode_wrote_and_refuses_truncated_input() {
    let big: Nat = "123456789012345678901234567890123456789".parse().unwrap();
    let mut encoded: Vec<u8> = encode(big.clone()).into_iter().collect();
    encoded.extend([7, 8]);
    assert_eq!(decode(bytes(&encoded)), Some((big.clone(), bytes(&[7, 8]))));
    assert_eq!(decode(bytes(&[])), None);
    assert_eq!(decode(bytes(&[0x80, 0xff, 0x81])), None, "every byte continues the number");
    let ns: List<Nat> = [0u64, 1, 127, 128, 16384, u64::MAX].into_iter().map(Nat::from).collect();
    assert_eq!(decode_all(encode_all(ns.clone())), Some(ns));
    assert_eq!(decode_all(bytes(&[0x01, 0x80])), None, "the last number is cut off");
}

/// TEST0316: the crate reports its round trips
#[test]
fn test0316_the_crate_reports_its_round_trips() {
    let a = __meta::assurance();
    let c = a.claim("Varint.decode_encode").unwrap();
    assert_eq!((c.relation, c.status), ("lungo.roundtrip", ClaimStatus::Proved));
    assert_eq!(c.subjects, ["Varint.encode", "Varint.decode"]);
    assert_eq!(a.claim("Varint.decodeAll_encodeAll").unwrap().subjects, ["Varint.encodeAll", "Varint.decodeAll"]);
    assert_eq!(
        __meta::declaration("Varint.decode").unwrap().assurance.claims,
        ["Varint.decode_encode", "Varint.decode_truncated"]
    );
}
