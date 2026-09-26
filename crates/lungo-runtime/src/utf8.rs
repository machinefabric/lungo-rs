//! UTF-8 helpers matching Lean's `runtime/utf8.cpp`.

use std::borrow::Cow;

/// Decodes `bytes`, replacing each maximal invalid sequence with U+FFFD in the same way as
/// Lean's `lean_mk_string_lossy_recover`: an invalid lead byte and the continuation bytes that
/// follow it are replaced by a single replacement character.
pub fn decode_lossy(bytes: &[u8]) -> Cow<'_, str> {
    if let Ok(s) = std::str::from_utf8(bytes) {
        return Cow::Borrowed(s);
    }
    let mut out = String::with_capacity(bytes.len() + 3);
    let mut pos = 0;
    let mut start = 0;
    while pos < bytes.len() {
        match validate_one(bytes, pos) {
            Some(next) => pos = next,
            None => {
                out.push_str(std::str::from_utf8(&bytes[start..pos]).expect("validated prefix"));
                out.push('\u{fffd}');
                pos += 1;
                while pos < bytes.len() && (bytes[pos] & 0xc0) == 0x80 {
                    pos += 1;
                }
                start = pos;
            }
        }
    }
    out.push_str(std::str::from_utf8(&bytes[start..pos]).expect("validated suffix"));
    Cow::Owned(out)
}

/// Validates one UTF-8 encoded scalar value at `pos`, returning the position after it.
/// Rejects overlong encodings, surrogates, and values above U+10FFFF, as Lean does.
pub fn validate_one(s: &[u8], pos: usize) -> Option<usize> {
    let c = *s.get(pos)?;
    let cont = |i: usize| s.get(pos + i).filter(|b| (**b & 0xc0) == 0x80).copied();
    if c & 0x80 == 0 {
        Some(pos + 1)
    } else if c & 0xe0 == 0xc0 {
        let c1 = cont(1)?;
        let r = ((c as u32 & 0x1f) << 6) | (c1 as u32 & 0x3f);
        (r >= 0x80).then_some(pos + 2)
    } else if c & 0xf0 == 0xe0 {
        let c1 = cont(1)?;
        let c2 = cont(2)?;
        let r = ((c as u32 & 0x0f) << 12) | ((c1 as u32 & 0x3f) << 6) | (c2 as u32 & 0x3f);
        (r >= 0x800 && !(0xd800..=0xdfff).contains(&r)).then_some(pos + 3)
    } else if c & 0xf8 == 0xf0 {
        let c1 = cont(1)?;
        let c2 = cont(2)?;
        let c3 = cont(3)?;
        let r = ((c as u32 & 0x07) << 18) | ((c1 as u32 & 0x3f) << 12) | ((c2 as u32 & 0x3f) << 6) | (c3 as u32 & 0x3f);
        (0x10000..=0x10ffff).contains(&r).then_some(pos + 4)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lossy_decoding_replaces_each_invalid_sequence_once() {
        assert_eq!(decode_lossy(b"ab\xffcd"), "ab\u{fffd}cd");
        assert_eq!(decode_lossy(b"\xe2\x82"), "\u{fffd}");
        // An overlong encoding of '/' and a surrogate are rejected.
        assert_eq!(decode_lossy(b"\xc0\xafx"), "\u{fffd}x");
        assert_eq!(decode_lossy(b"\xed\xa0\x80"), "\u{fffd}");
        assert_eq!(decode_lossy("héllo".as_bytes()), "héllo");
    }
}
