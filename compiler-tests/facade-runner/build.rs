use lungo_build::Mode;

fn main() -> lungo_build::Result<()> {
    println!("cargo::rustc-check-cfg=cfg(lean_oracle)");
    // LeanOracle links Lean's native runtime, which Lean builds for the GNU ABI on Windows
    // (LNG0104). On an MSVC target there is no oracle to compare with, so neither mode is
    // generated and the comparison is not compiled.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let target_env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    if target_os == "windows" && target_env == "msvc" {
        println!(
            "cargo::warning=the PureRust/LeanOracle comparison is not built for an MSVC target: \
             LeanOracle needs a `*-windows-gnu` target (LNG0104)"
        );
        return Ok(());
    }
    println!("cargo::rustc-cfg=lean_oracle");
    for (name, mode) in [("pure", Mode::PureRust), ("oracle", Mode::LeanOracle)] {
        lungo_build::configure().mode(mode).name(name).compile_lean("../facade")?;
    }
    Ok(())
}
