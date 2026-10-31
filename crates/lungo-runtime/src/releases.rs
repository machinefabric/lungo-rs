//! The Lean releases whose compiler output lungo translates, and what differs between them.
//!
//! One runtime serves every supported release. A primitive whose behaviour a release changed is
//! implemented once per behaviour, each implementation declaring the releases it serves
//! ([`Releases`]), and the code generator binds each extern call of a program to the
//! implementation serving the program's release. What a program reports of its own Lean release
//! (`Lean.versionString`, `Lean.githash`) is compiled into the program ([`toolchain_constant`]),
//! so programs compiled from different releases can share one runtime in one process.

use crate::registry::Ty;
use std::fmt;

/// A Lean release number, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LeanVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl LeanVersion {
    pub const fn new(major: u32, minor: u32, patch: u32) -> LeanVersion {
        LeanVersion { major, minor, patch }
    }

    /// Parses `4.35.0`: exactly three decimal parts, as Lean's release numbers are written.
    pub fn parse(text: &str) -> Option<LeanVersion> {
        let mut parts = text.split('.');
        let mut next = || -> Option<u32> {
            let p = parts.next()?;
            if p.is_empty() || !p.bytes().all(|b| b.is_ascii_digit()) || (p.len() > 1 && p.starts_with('0')) {
                return None;
            }
            p.parse().ok()
        };
        let v = LeanVersion::new(next()?, next()?, next()?);
        if parts.next().is_some() { None } else { Some(v) }
    }
}

impl fmt::Display for LeanVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// A supported Lean release: its number and the commit its release build was made from.
#[derive(Debug, PartialEq, Eq)]
pub struct LeanRelease {
    pub version: LeanVersion,
    pub githash: &'static str,
}

impl LeanRelease {
    /// The `lean-toolchain` pin naming this release.
    pub fn pin(&self) -> String {
        format!("leanprover/lean4:v{}", self.version)
    }
}

/// Every supported Lean release, oldest first. 4.34.1 is the first; every stable release after it
/// is supported, and none is dropped.
pub const LEAN_RELEASES: &[LeanRelease] = &[
    LeanRelease { version: LeanVersion::new(4, 34, 1), githash: "5045d0056413266e57c625dcd7c365b10e377c52" },
    LeanRelease { version: LeanVersion::new(4, 35, 0), githash: "eb6cafb7290e8bd5f2937522d8d804f58f02bafc" },
];

/// The supported release numbered `version`.
pub fn release(version: LeanVersion) -> Option<&'static LeanRelease> {
    LEAN_RELEASES.iter().find(|r| r.version == version)
}

/// The newest supported release.
pub fn newest() -> &'static LeanRelease {
    LEAN_RELEASES.last().expect("lungo supports at least one Lean release")
}

/// The releases a primitive's implementation serves: from `since` through `until`, both
/// inclusive; an absent bound is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Releases {
    pub since: Option<LeanVersion>,
    pub until: Option<LeanVersion>,
}

impl Releases {
    pub const ALL: Releases = Releases { since: None, until: None };

    pub fn contains(&self, version: LeanVersion) -> bool {
        self.since.is_none_or(|s| s <= version) && self.until.is_none_or(|u| version <= u)
    }
}

/// A value of the program's Lean release, reported through an extern of Lean's runtime and
/// compiled into the program.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolchainConstant {
    Nat(u32),
    Bool(bool),
    String(&'static str),
}

impl ToolchainConstant {
    /// The representation of the extern reporting it: `Unit → Nat`, `Unit → Bool`, `Unit → String`.
    pub fn representation(self) -> (&'static [Ty], Ty) {
        match self {
            ToolchainConstant::Nat(_) | ToolchainConstant::String(_) => (&[Ty::obj], Ty::obj),
            ToolchainConstant::Bool(_) => (&[Ty::obj], Ty::u8),
        }
    }
}

/// The externs whose values are those of the program's Lean release.
pub const TOOLCHAIN_CONSTANT_SYMBOLS: &[&str] = &[
    "lean_version_get_major",
    "lean_version_get_minor",
    "lean_version_get_patch",
    "lean_version_get_is_release",
    "lean_version_get_special_desc",
    "lean_get_githash",
];

/// The value the extern `symbol` reports for a program compiled from `release`, if it reports one
/// of the release's. Every supported release is a release build without a special description.
pub fn toolchain_constant(symbol: &str, release: &LeanRelease) -> Option<ToolchainConstant> {
    Some(match symbol {
        "lean_version_get_major" => ToolchainConstant::Nat(release.version.major),
        "lean_version_get_minor" => ToolchainConstant::Nat(release.version.minor),
        "lean_version_get_patch" => ToolchainConstant::Nat(release.version.patch),
        "lean_version_get_is_release" => ToolchainConstant::Bool(true),
        "lean_version_get_special_desc" => ToolchainConstant::String(""),
        "lean_get_githash" => ToolchainConstant::String(release.githash),
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// TEST0377: Lean release numbers are read exactly and ordered numerically
    #[test]
    fn test0377_lean_release_numbers_are_read_exactly_and_ordered_numerically() {
        assert_eq!(LeanVersion::parse("4.35.0"), Some(LeanVersion::new(4, 35, 0)));
        for bad in ["4.35", "4.35.0.1", "4.35.0-rc3", "v4.35.0", "4..0", "4.035.0", " 4.35.0", ""] {
            assert_eq!(LeanVersion::parse(bad), None, "{bad:?}");
        }
        // Numeric, not textual: 4.100.0 follows 4.35.0.
        assert!(LeanVersion::new(4, 35, 0) < LeanVersion::new(4, 100, 0));
        assert!(LeanVersion::new(4, 34, 1) < LeanVersion::new(4, 35, 0));
        assert_eq!(LeanVersion::new(4, 35, 0).to_string(), "4.35.0");
    }

    /// TEST0378: the supported releases are listed oldest first, once each, from 4.34.1
    #[test]
    fn test0378_the_supported_releases_are_listed_oldest_first_once_each_from_4_34_1() {
        assert_eq!(LEAN_RELEASES[0].version, LeanVersion::new(4, 34, 1));
        for pair in LEAN_RELEASES.windows(2) {
            assert!(pair[0].version < pair[1].version, "{} is listed before {}", pair[0].version, pair[1].version);
        }
        for r in LEAN_RELEASES {
            assert_eq!(r.githash.len(), 40, "{}", r.version);
            assert!(r.githash.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()), "{}", r.version);
            assert_eq!(release(r.version), Some(r));
        }
        assert_eq!(release(LeanVersion::new(4, 34, 0)), None);
    }

    /// TEST0379: a bounded implementation serves exactly the releases within its bounds
    #[test]
    fn test0379_a_bounded_implementation_serves_exactly_the_releases_within_its_bounds() {
        let until = Releases { since: None, until: Some(LeanVersion::new(4, 34, 1)) };
        let since = Releases { since: Some(LeanVersion::new(4, 35, 0)), until: None };
        let v34 = LeanVersion::new(4, 34, 1);
        let v35 = LeanVersion::new(4, 35, 0);
        assert!(until.contains(v34) && !until.contains(v35));
        assert!(!since.contains(v34) && since.contains(v35));
        assert!(Releases::ALL.contains(v34) && Releases::ALL.contains(v35));
    }

    /// TEST0380: a program reports the release it was compiled from
    #[test]
    fn test0380_a_program_reports_the_release_it_was_compiled_from() {
        let old = release(LeanVersion::new(4, 34, 1)).unwrap();
        let new = release(LeanVersion::new(4, 35, 0)).unwrap();
        assert_eq!(toolchain_constant("lean_version_get_minor", old), Some(ToolchainConstant::Nat(34)));
        assert_eq!(toolchain_constant("lean_version_get_minor", new), Some(ToolchainConstant::Nat(35)));
        assert_eq!(toolchain_constant("lean_version_get_patch", old), Some(ToolchainConstant::Nat(1)));
        assert_eq!(
            toolchain_constant("lean_get_githash", old),
            Some(ToolchainConstant::String("5045d0056413266e57c625dcd7c365b10e377c52"))
        );
        assert_eq!(toolchain_constant("lean_nat_add", new), None);
        for s in TOOLCHAIN_CONSTANT_SYMBOLS {
            assert!(toolchain_constant(s, new).is_some(), "{s}");
        }
    }
}
