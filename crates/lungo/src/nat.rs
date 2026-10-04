use num_bigint::BigUint;
use num_traits::ToPrimitive;
use std::fmt;
use std::str::FromStr;

/// A Lean natural number: an arbitrary-precision non-negative integer.
#[derive(Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Nat(Repr);

/// Values that fit a `u64` are stored inline; the invariant `Big(v)` implies `v > u64::MAX`
/// keeps the representation canonical.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Repr {
    Small(u64),
    Big(BigUint),
}

impl PartialOrd for Repr {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Repr {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        match (self, other) {
            (Repr::Small(a), Repr::Small(b)) => a.cmp(b),
            (Repr::Small(_), Repr::Big(_)) => std::cmp::Ordering::Less,
            (Repr::Big(_), Repr::Small(_)) => std::cmp::Ordering::Greater,
            (Repr::Big(a), Repr::Big(b)) => a.cmp(b),
        }
    }
}

impl Nat {
    pub const ZERO: Nat = Nat(Repr::Small(0));

    pub fn from_biguint(v: BigUint) -> Nat {
        match v.to_u64() {
            Some(s) => Nat(Repr::Small(s)),
            None => Nat(Repr::Big(v)),
        }
    }

    pub fn to_biguint(&self) -> BigUint {
        match &self.0 {
            Repr::Small(v) => BigUint::from(*v),
            Repr::Big(v) => v.clone(),
        }
    }

    /// The value as a `u64`, if it fits.
    pub fn to_u64(&self) -> Option<u64> {
        match &self.0 {
            Repr::Small(v) => Some(*v),
            Repr::Big(_) => None,
        }
    }

    pub fn is_zero(&self) -> bool {
        matches!(self.0, Repr::Small(0))
    }
}

impl Default for Nat {
    fn default() -> Self {
        Nat::ZERO
    }
}

macro_rules! from_unsigned {
    ($($t:ty),*) => {$(
        impl From<$t> for Nat {
            fn from(v: $t) -> Nat {
                Nat(Repr::Small(v as u64))
            }
        }
    )*};
}
from_unsigned!(u8, u16, u32, u64, usize);

impl From<u128> for Nat {
    fn from(v: u128) -> Nat {
        Nat::from_biguint(BigUint::from(v))
    }
}

impl From<BigUint> for Nat {
    fn from(v: BigUint) -> Nat {
        Nat::from_biguint(v)
    }
}

impl From<Nat> for BigUint {
    fn from(v: Nat) -> BigUint {
        match v.0 {
            Repr::Small(s) => BigUint::from(s),
            Repr::Big(b) => b,
        }
    }
}

/// Error returned when a [`Nat`] does not fit the requested integer type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatOutOfRange;

impl fmt::Display for NatOutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("natural number out of range for the target integer type")
    }
}

impl std::error::Error for NatOutOfRange {}

macro_rules! try_into_unsigned {
    ($($t:ty),*) => {$(
        impl TryFrom<&Nat> for $t {
            type Error = NatOutOfRange;
            fn try_from(v: &Nat) -> Result<$t, NatOutOfRange> {
                v.to_u64().and_then(|s| <$t>::try_from(s).ok()).ok_or(NatOutOfRange)
            }
        }
        impl TryFrom<Nat> for $t {
            type Error = NatOutOfRange;
            fn try_from(v: Nat) -> Result<$t, NatOutOfRange> {
                <$t>::try_from(&v)
            }
        }
    )*};
}
try_into_unsigned!(u8, u16, u32, u64, usize);

impl fmt::Display for Nat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Small(v) => fmt::Display::fmt(v, f),
            Repr::Big(v) => fmt::Display::fmt(v, f),
        }
    }
}

impl fmt::Debug for Nat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

/// Error returned when parsing a [`Nat`] from a string that is not a decimal numeral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseNatError;

impl fmt::Display for ParseNatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("invalid natural number literal")
    }
}

impl std::error::Error for ParseNatError {}

impl FromStr for Nat {
    type Err = ParseNatError;
    fn from_str(s: &str) -> Result<Nat, ParseNatError> {
        if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
            return Err(ParseNatError);
        }
        BigUint::from_str(s).map(Nat::from_biguint).map_err(|_| ParseNatError)
    }
}

impl std::ops::Add for &Nat {
    type Output = Nat;
    fn add(self, rhs: &Nat) -> Nat {
        match (&self.0, &rhs.0) {
            (Repr::Small(a), Repr::Small(b)) => match a.checked_add(*b) {
                Some(s) => Nat(Repr::Small(s)),
                None => Nat::from_biguint(BigUint::from(*a) + BigUint::from(*b)),
            },
            _ => Nat::from_biguint(self.to_biguint() + rhs.to_biguint()),
        }
    }
}

impl std::ops::Add for Nat {
    type Output = Nat;
    fn add(self, rhs: Nat) -> Nat {
        &self + &rhs
    }
}

impl std::ops::Mul for &Nat {
    type Output = Nat;
    fn mul(self, rhs: &Nat) -> Nat {
        match (&self.0, &rhs.0) {
            (Repr::Small(a), Repr::Small(b)) => match a.checked_mul(*b) {
                Some(s) => Nat(Repr::Small(s)),
                None => Nat::from_biguint(BigUint::from(*a) * BigUint::from(*b)),
            },
            _ => Nat::from_biguint(self.to_biguint() * rhs.to_biguint()),
        }
    }
}

impl std::ops::Mul for Nat {
    type Output = Nat;
    fn mul(self, rhs: Nat) -> Nat {
        &self * &rhs
    }
}

impl Nat {
    /// Truncated subtraction, as Lean's `Nat.sub`: `a - b = 0` when `b > a`.
    pub fn saturating_sub(&self, rhs: &Nat) -> Nat {
        if self <= rhs {
            return Nat::ZERO;
        }
        match (&self.0, &rhs.0) {
            (Repr::Small(a), Repr::Small(b)) => Nat(Repr::Small(a - b)),
            _ => Nat::from_biguint(self.to_biguint() - rhs.to_biguint()),
        }
    }
}

impl num_traits::Zero for Nat {
    fn zero() -> Self {
        Nat::ZERO
    }
    fn is_zero(&self) -> bool {
        Nat::is_zero(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0253: representation is canonical across the u64 boundary
    #[test]
    fn test0253_representation_is_canonical_across_the_u64_boundary() {
        let max = Nat::from(u64::MAX);
        let over = &max + &Nat::from(1u8);
        assert_eq!(over.to_u64(), None);
        assert_eq!(over.to_string(), "18446744073709551616");
        assert_eq!(over.saturating_sub(&Nat::from(1u8)), max);
        assert_eq!("18446744073709551615".parse::<Nat>().unwrap().to_u64(), Some(u64::MAX));
        assert!("-1".parse::<Nat>().is_err());
        assert!(Nat::from(3u8) < over);
        assert_eq!(Nat::from(2u8).saturating_sub(&Nat::from(5u8)), Nat::ZERO);
    }
}
