//! Layout fingerprints: how a crate using another crate's generated type checks that both were
//! generated from the same Lean definition.

/// A Rust type standing for a Lean type whose runtime layout the program depends on.
///
/// Generated types implement it. A type an application provides for a Lean type (lungo's
/// `extern_type`) must implement it too, with the fingerprint lungo reports for that Lean type:
/// every program using the type checks, when it is compiled, that the fingerprint it was generated
/// for is this one, so a type whose definition changed on one side only is a compile error rather
/// than values read at the wrong layout.
pub trait LeanLayout {
    /// The layout fingerprint (lowercase hexadecimal SHA-256) of the Lean type.
    const FINGERPRINT: &'static str;
}

impl<T: LeanLayout + ?Sized, B: crate::Backend> LeanLayout for crate::LeanValue<T, B> {
    const FINGERPRINT: &'static str = T::FINGERPRINT;
}

/// Fails the compilation of a program generated for a layout of `T`'s Lean type other than `T`'s.
#[doc(hidden)]
pub const fn assert_layout<T: LeanLayout + ?Sized>(expected: &str) {
    let (a, b) = (T::FINGERPRINT.as_bytes(), expected.as_bytes());
    let mut same = a.len() == b.len();
    let mut i = 0;
    while same && i < a.len() {
        same = a[i] == b[i];
        i += 1;
    }
    if !same {
        panic!(
            "a type provided from another crate has a different Lean layout than the one this program was generated for: regenerate both from the same Lean definition"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Point;
    impl LeanLayout for Point {
        const FINGERPRINT: &'static str = "ab12";
    }

    #[test]
    fn the_layout_a_program_was_generated_for_must_be_the_types() {
        assert_layout::<Point>("ab12");
        assert_layout::<crate::LeanValue<Point>>("ab12");
        for other in ["ab13", "ab1", "ab123", ""] {
            let refused = std::panic::catch_unwind(|| assert_layout::<Point>(other));
            assert!(refused.is_err(), "{other:?} accepted for ab12");
        }
    }
}
