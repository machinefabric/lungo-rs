fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        // The clock facility, implemented by this crate.
        .rust_extern("timing_ticks_per_second", "crate::host::ticks_per_second")
        .rust_extern("timing_now", "crate::host::now")
        .require_claims("Timing.toTicks")
        .require_claims("Timing.toSeconds")
        .require_claims("Timing.secondsLeft")
        .compile_lean("lean")
}
