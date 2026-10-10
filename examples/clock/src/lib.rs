//! Deadlines on a clock this crate provides, compiled from Lean. The claims about them hold on a
//! clock that ticks (`Timing.Ticks`): an assumption about [`host`], which Lean cannot check.

lungo::include_lean!("timing");

/// The clock facility: a clock of a thousand ticks per second, which tests step by hand.
///
/// `ticks_per_second` has a pure Lean type, so it must answer the same every time: Lean may
/// evaluate `ticksPerSecond ()` once and keep the value.
pub mod host {
    use lungo::{IoError, Nat};
    use std::sync::atomic::{AtomicU64, Ordering};

    static NOW: AtomicU64 = AtomicU64::new(0);

    /// Advances the clock by `ticks`.
    pub fn advance(ticks: u64) {
        NOW.fetch_add(ticks, Ordering::SeqCst);
    }

    pub fn ticks_per_second(_u: ()) -> Nat {
        Nat::from(1000u64)
    }

    pub fn now(_u: ()) -> Result<Nat, IoError> {
        Ok(Nat::from(NOW.load(Ordering::SeqCst)))
    }
}
