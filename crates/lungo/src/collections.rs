use std::ops::{Deref, DerefMut};

/// A Lean `List α`, exposed as a contiguous vector.
///
/// Lean's `Array α` is exposed as `Vec<T>`; `List` is a distinct type so that each Lean type has
/// exactly one Rust representation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct List<T>(pub Vec<T>);

impl<T> List<T> {
    pub fn new() -> Self {
        List(Vec::new())
    }

    pub fn into_vec(self) -> Vec<T> {
        self.0
    }
}

impl<T> From<Vec<T>> for List<T> {
    fn from(v: Vec<T>) -> Self {
        List(v)
    }
}

impl<T> From<List<T>> for Vec<T> {
    fn from(v: List<T>) -> Self {
        v.0
    }
}

impl<T> FromIterator<T> for List<T> {
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        List(iter.into_iter().collect())
    }
}

impl<T> IntoIterator for List<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a List<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl<T> Deref for List<T> {
    type Target = Vec<T>;
    fn deref(&self) -> &Vec<T> {
        &self.0
    }
}

impl<T> DerefMut for List<T> {
    fn deref_mut(&mut self) -> &mut Vec<T> {
        &mut self.0
    }
}

/// A Lean `ByteArray`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ByteArray(pub Vec<u8>);

impl From<Vec<u8>> for ByteArray {
    fn from(v: Vec<u8>) -> Self {
        ByteArray(v)
    }
}

impl Deref for ByteArray {
    type Target = Vec<u8>;
    fn deref(&self) -> &Vec<u8> {
        &self.0
    }
}

impl DerefMut for ByteArray {
    fn deref_mut(&mut self) -> &mut Vec<u8> {
        &mut self.0
    }
}

/// A Lean `FloatArray`.
#[derive(Clone, Debug, Default, PartialEq, PartialOrd)]
pub struct FloatArray(pub Vec<f64>);

impl From<Vec<f64>> for FloatArray {
    fn from(v: Vec<f64>) -> Self {
        FloatArray(v)
    }
}

impl Deref for FloatArray {
    type Target = Vec<f64>;
    fn deref(&self) -> &Vec<f64> {
        &self.0
    }
}

impl DerefMut for FloatArray {
    fn deref_mut(&mut self) -> &mut Vec<f64> {
        &mut self.0
    }
}
