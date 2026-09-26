use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::fmt;
use std::str::FromStr;

/// A Lean integer: an arbitrary-precision signed integer.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Int(Repr);

/// Values that fit an `i64` are stored inline; `Big(v)` implies `v` is outside the `i64` range.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Small(i64),
    Big(BigInt),
}

impl Int {
    pub const ZERO: Int = Int(Repr::Small(0));

    pub fn from_bigint(v: BigInt) -> Int {
        match v.to_i64() {
            Some(s) => Int(Repr::Small(s)),
            None => Int(Repr::Big(v)),
        }
    }

    pub fn to_bigint(&self) -> BigInt {
        match &self.0 {
            Repr::Small(v) => BigInt::from(*v),
            Repr::Big(v) => v.clone(),
        }
    }

    pub fn to_i64(&self) -> Option<i64> {
        match &self.0 {
            Repr::Small(v) => Some(*v),
            Repr::Big(_) => None,
        }
    }
}

impl Default for Int {
    fn default() -> Self {
        Int::ZERO
    }
}

impl PartialOrd for Int {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Int {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (&self.0, &other.0) {
            (Repr::Small(a), Repr::Small(b)) => a.cmp(b),
            _ => self.to_bigint().cmp(&other.to_bigint()),
        }
    }
}

macro_rules! from_int {
    ($($t:ty),*) => {$(
        impl From<$t> for Int {
            fn from(v: $t) -> Int {
                Int(Repr::Small(v as i64))
            }
        }
    )*};
}
from_int!(i8, i16, i32, i64, u8, u16, u32);

impl From<u64> for Int {
    fn from(v: u64) -> Int {
        Int::from_bigint(BigInt::from(v))
    }
}

impl From<i128> for Int {
    fn from(v: i128) -> Int {
        Int::from_bigint(BigInt::from(v))
    }
}

impl From<BigInt> for Int {
    fn from(v: BigInt) -> Int {
        Int::from_bigint(v)
    }
}

impl From<crate::Nat> for Int {
    fn from(v: crate::Nat) -> Int {
        Int::from_bigint(BigInt::from(num_bigint::BigUint::from(v)))
    }
}

impl From<Int> for BigInt {
    fn from(v: Int) -> BigInt {
        match v.0 {
            Repr::Small(s) => BigInt::from(s),
            Repr::Big(b) => b,
        }
    }
}

/// Error returned when an [`Int`] does not fit the requested integer type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntOutOfRange;

impl fmt::Display for IntOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("integer out of range for the target integer type")
    }
}

impl std::error::Error for IntOutOfRange {}

macro_rules! try_into_int {
    ($($t:ty),*) => {$(
        impl TryFrom<&Int> for $t {
            type Error = IntOutOfRange;
            fn try_from(v: &Int) -> Result<$t, IntOutOfRange> {
                v.to_i64().and_then(|s| <$t>::try_from(s).ok()).ok_or(IntOutOfRange)
            }
        }
    )*};
}
try_into_int!(i8, i16, i32, i64, isize, u8, u16, u32, u64, usize);

impl fmt::Display for Int {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Small(v) => fmt::Display::fmt(v, f),
            Repr::Big(v) => fmt::Display::fmt(v, f),
        }
    }
}

impl fmt::Debug for Int {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Error returned when parsing an [`Int`] from a string that is not a decimal numeral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseIntError;

impl fmt::Display for ParseIntError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid integer literal")
    }
}

impl std::error::Error for ParseIntError {}

impl FromStr for Int {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<Int, ParseIntError> {
        let digits = s.strip_prefix('-').unwrap_or(s);
        if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseIntError);
        }
        BigInt::from_str(s).map(Int::from_bigint).map_err(|_| ParseIntError)
    }
}

impl std::ops::Neg for &Int {
    type Output = Int;
    fn neg(self) -> Int {
        match &self.0 {
            Repr::Small(v) => match v.checked_neg() {
                Some(n) => Int(Repr::Small(n)),
                None => Int::from_bigint(-BigInt::from(*v)),
            },
            Repr::Big(v) => Int::from_bigint(-v.clone()),
        }
    }
}

impl std::ops::Add for &Int {
    type Output = Int;
    fn add(self, rhs: &Int) -> Int {
        match (&self.0, &rhs.0) {
            (Repr::Small(a), Repr::Small(b)) => match a.checked_add(*b) {
                Some(s) => Int(Repr::Small(s)),
                None => Int::from_bigint(BigInt::from(*a) + BigInt::from(*b)),
            },
            _ => Int::from_bigint(self.to_bigint() + rhs.to_bigint()),
        }
    }
}

impl std::ops::Sub for &Int {
    type Output = Int;
    fn sub(self, rhs: &Int) -> Int {
        self + &(-rhs)
    }
}

impl std::ops::Mul for &Int {
    type Output = Int;
    fn mul(self, rhs: &Int) -> Int {
        match (&self.0, &rhs.0) {
            (Repr::Small(a), Repr::Small(b)) => match a.checked_mul(*b) {
                Some(s) => Int(Repr::Small(s)),
                None => Int::from_bigint(BigInt::from(*a) * BigInt::from(*b)),
            },
            _ => Int::from_bigint(self.to_bigint() * rhs.to_bigint()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn representation_is_canonical_across_the_i64_boundary() {
        let min = Int::from(i64::MIN);
        let below = &min - &Int::from(1);
        assert_eq!(below.to_i64(), None);
        assert_eq!(below.to_string(), "-9223372036854775809");
        assert_eq!((&below + &Int::from(1)).to_i64(), Some(i64::MIN));
        assert_eq!((-&min).to_string(), "9223372036854775808");
        assert!(below < min);
        assert_eq!("-12".parse::<Int>().unwrap(), Int::from(-12));
        assert!("--1".parse::<Int>().is_err());
    }
}
