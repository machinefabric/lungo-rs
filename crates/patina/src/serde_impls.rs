//! `serde` support for the facade types (feature `serde`).
//!
//! `Nat` and `Int` are unbounded, which formats such as JSON cannot represent as numbers without
//! loss, so they serialize as decimal strings (`"12"`, `"-3"`). They deserialize from such a
//! string or from an integer. `List` serializes as a sequence, `ByteArray` as bytes and
//! `FloatArray` as a sequence of floats.

use crate::{ByteArray, FloatArray, Int, List, Nat};
use serde::de::{self, Deserialize, Deserializer, Visitor};
use serde::ser::{Serialize, Serializer};
use std::fmt;

impl Serialize for Nat {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Nat {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct NatVisitor;
        impl Visitor<'_> for NatVisitor {
            type Value = Nat;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a natural number, as a decimal string or an unsigned integer")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Nat, E> {
                v.parse().map_err(|_| E::invalid_value(de::Unexpected::Str(v), &self))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Nat, E> {
                Ok(Nat::from(v))
            }
            fn visit_u128<E: de::Error>(self, v: u128) -> Result<Nat, E> {
                Ok(Nat::from(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Nat, E> {
                u64::try_from(v).map(Nat::from).map_err(|_| E::invalid_value(de::Unexpected::Signed(v), &self))
            }
        }
        d.deserialize_any(NatVisitor)
    }
}

impl Serialize for Int {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Int {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct IntVisitor;
        impl Visitor<'_> for IntVisitor {
            type Value = Int;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("an integer, as a decimal string or an integer")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Int, E> {
                v.parse().map_err(|_| E::invalid_value(de::Unexpected::Str(v), &self))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Int, E> {
                Ok(Int::from(v))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Int, E> {
                Ok(Int::from(v as i128))
            }
            fn visit_i128<E: de::Error>(self, v: i128) -> Result<Int, E> {
                Ok(Int::from(v))
            }
        }
        d.deserialize_any(IntVisitor)
    }
}

impl<T: Serialize> Serialize for List<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for List<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::deserialize(d).map(List)
    }
}

impl Serialize for ByteArray {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_bytes(&self.0)
    }
}

impl<'de> Deserialize<'de> for ByteArray {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct BytesVisitor;
        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = ByteArray;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bytes")
            }
            fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<ByteArray, E> {
                Ok(ByteArray(v.to_vec()))
            }
            fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<ByteArray, E> {
                Ok(ByteArray(v))
            }
            fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<ByteArray, A::Error> {
                let mut out = Vec::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(b) = seq.next_element::<u8>()? {
                    out.push(b);
                }
                Ok(ByteArray(out))
            }
        }
        d.deserialize_byte_buf(BytesVisitor)
    }
}

impl Serialize for FloatArray {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

impl<'de> Deserialize<'de> for FloatArray {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Vec::deserialize(d).map(FloatArray)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unbounded_numbers_round_trip_as_decimal_strings() {
        let big: Nat = "340282366920938463463374607431768211456".parse().unwrap();
        let json = serde_json::to_string(&big).unwrap();
        assert_eq!(json, "\"340282366920938463463374607431768211456\"");
        assert_eq!(serde_json::from_str::<Nat>(&json).unwrap(), big);
        assert_eq!(serde_json::from_str::<Nat>("7").unwrap(), Nat::from(7u64));
        assert!(serde_json::from_str::<Nat>("-1").is_err(), "a negative number is no natural number");
        assert!(serde_json::from_str::<Nat>("\"1x\"").is_err());
        let neg: Int = "-12345678901234567890123".parse().unwrap();
        let json = serde_json::to_string(&neg).unwrap();
        assert_eq!(serde_json::from_str::<Int>(&json).unwrap(), neg);
        assert_eq!(serde_json::from_str::<Int>("-5").unwrap(), Int::from(-5i128));
    }

    #[test]
    fn collections_serialize_as_sequences() {
        let l = List(vec![Nat::from(1u64), Nat::from(2u64)]);
        let json = serde_json::to_string(&l).unwrap();
        assert_eq!(json, "[\"1\",\"2\"]");
        assert_eq!(serde_json::from_str::<List<Nat>>(&json).unwrap(), l);
        let b = ByteArray(vec![0, 255]);
        assert_eq!(serde_json::from_str::<ByteArray>(&serde_json::to_string(&b).unwrap()).unwrap(), b);
        let f = FloatArray(vec![0.5, -1.0]);
        assert_eq!(serde_json::from_str::<FloatArray>(&serde_json::to_string(&f).unwrap()).unwrap(), f);
    }
}
