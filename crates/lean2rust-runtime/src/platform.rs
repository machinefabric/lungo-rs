//! Platform and version queries, ported from Lean's `runtime/platform.cpp` and `lean.h`.
//!
//! Version values are those of the Lean release whose compiler output the bridge translates
//! (Lean 4.34.1, commit 5045d0056413266e57c625dcd7c365b10e377c52, a release build without the
//! LLVM backend). Platform values describe the Rust target the program is compiled for.

use crate::object::*;
use crate::registry::Unsupported;

pub(crate) const UNSUPPORTED: &[Unsupported] = &[];

pub const LEAN_VERSION_MAJOR: usize = 4;
pub const LEAN_VERSION_MINOR: usize = 34;
pub const LEAN_VERSION_PATCH: usize = 1;
pub const LEAN_VERSION_IS_RELEASE: bool = true;
pub const LEAN_SPECIAL_VERSION_DESC: &str = "";
pub const LEAN_GITHASH: &str = "5045d0056413266e57c625dcd7c365b10e377c52";

/// The target triple in the spelling Lean's `LEAN_PLATFORM_TARGET` uses (the Clang target
/// triple): Apple's 64-bit ARM architecture is `arm64` and macOS is `darwin`. The OS version
/// suffix Clang appends on Darwin describes the build host of a native Lean build and has no
/// counterpart for a cross-compiled Rust target, so it is omitted.
pub fn platform_target() -> String {
    let arch =
        if cfg!(all(target_arch = "aarch64", target_vendor = "apple")) { "arm64" } else { std::env::consts::ARCH };
    let vendor = if cfg!(target_vendor = "apple") {
        "apple"
    } else if cfg!(target_vendor = "pc") {
        "pc"
    } else {
        "unknown"
    };
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let env = if cfg!(target_env = "gnu") {
        "-gnu"
    } else if cfg!(target_env = "musl") {
        "-musl"
    } else if cfg!(target_env = "msvc") {
        "-msvc"
    } else {
        ""
    };
    format!("{arch}-{vendor}-{os}{env}")
}

pub mod externs {
    use super::*;

    crate::lean_externs! {
        fn lean_system_platform_nbits(_unit: obj) -> obj {
            lean_box(usize::BITS as usize)
        }

        fn lean_system_platform_windows(_unit: obj) -> u8 {
            cfg!(windows) as u8
        }

        fn lean_system_platform_osx(_unit: obj) -> u8 {
            cfg!(target_os = "macos") as u8
        }

        fn lean_system_platform_linux(_unit: obj) -> u8 {
            cfg!(target_os = "linux") as u8
        }

        fn lean_system_platform_emscripten(_unit: obj) -> u8 {
            cfg!(target_os = "emscripten") as u8
        }

        fn lean_system_platform_target(_unit: obj) -> obj {
            lean_mk_string(&platform_target())
        }

        fn lean_version_get_major(_unit: obj) -> obj {
            lean_box(LEAN_VERSION_MAJOR)
        }

        fn lean_version_get_minor(_unit: obj) -> obj {
            lean_box(LEAN_VERSION_MINOR)
        }

        fn lean_version_get_patch(_unit: obj) -> obj {
            lean_box(LEAN_VERSION_PATCH)
        }

        fn lean_version_get_is_release(_unit: obj) -> u8 {
            LEAN_VERSION_IS_RELEASE as u8
        }

        fn lean_version_get_special_desc(_unit: obj) -> obj {
            lean_mk_string(LEAN_SPECIAL_VERSION_DESC)
        }

        fn lean_get_githash(_unit: obj) -> obj {
            lean_mk_string(LEAN_GITHASH)
        }

        fn lean_internal_is_stage0(_unit: obj) -> u8 {
            0
        }

        fn lean_internal_has_llvm_backend(_unit: obj) -> u8 {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::externs::*;
    use crate::object::*;

    #[test]
    fn values_match_the_lean_release() {
        unsafe {
            assert_eq!(lean_unbox(lean_system_platform_nbits(lean_box(0))), usize::BITS as usize);
            assert_eq!(lean_unbox(lean_version_get_major(lean_box(0))), 4);
            assert_eq!(lean_unbox(lean_version_get_minor(lean_box(0))), 34);
            assert_eq!(lean_unbox(lean_version_get_patch(lean_box(0))), 1);
            let h = lean_get_githash(lean_box(0));
            assert_eq!(lean_string_str(h), "5045d0056413266e57c625dcd7c365b10e377c52");
            lean_dec(h);
            assert_eq!(lean_internal_has_llvm_backend(lean_box(0)), 0);
            assert_eq!(lean_internal_is_stage0(lean_box(0)), 0);
            let exactly_one = lean_system_platform_linux(lean_box(0))
                + lean_system_platform_osx(lean_box(0))
                + lean_system_platform_windows(lean_box(0));
            assert_eq!(exactly_one, 1);
        }
    }

    #[cfg(all(target_arch = "aarch64", target_os = "macos"))]
    #[test]
    fn apple_silicon_target_uses_clang_spelling() {
        // Lean 4.34.1 on this platform reports "arm64-apple-darwin24.6.0".
        assert_eq!(super::platform_target(), "arm64-apple-darwin");
    }
}
