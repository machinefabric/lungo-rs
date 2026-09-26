//! Domain-separated, length-prefixed content hashing for build keys and cache identities.

pub struct Hasher(blake3::Hasher);

impl Hasher {
    pub fn new(domain: &str) -> Self {
        let mut h = blake3::Hasher::new();
        h.update(b"lean2rust:");
        h.update(&(domain.len() as u64).to_le_bytes());
        h.update(domain.as_bytes());
        Hasher(h)
    }

    pub fn field(&mut self, bytes: &[u8]) -> &mut Self {
        self.0.update(&(bytes.len() as u64).to_le_bytes());
        self.0.update(bytes);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.field(s.as_bytes())
    }

    pub fn finish(&self) -> String {
        self.0.finalize().to_hex().to_string()
    }
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fields_are_length_prefixed() {
        let mut a = Hasher::new("t");
        a.str("ab").str("c");
        let mut b = Hasher::new("t");
        b.str("a").str("bc");
        assert_ne!(a.finish(), b.finish());
        let mut c = Hasher::new("u");
        c.str("ab").str("c");
        assert_ne!(a.finish(), c.finish());
    }
}
