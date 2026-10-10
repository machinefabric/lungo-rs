use clock::{__meta, deadline_in, host, remaining, to_seconds, to_ticks};
use lungo::{ClaimStatus, Nat};
use std::sync::Mutex;

/// The clock is global: tests stepping it run one at a time.
static CLOCK: Mutex<()> = Mutex::new(());

fn nat(n: u64) -> Nat {
    Nat::from(n)
}

/// TEST0320: deadlines count down on a ticking clock
#[test]
fn test0320_deadlines_count_down_on_a_ticking_clock() {
    let _clock = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    let deadline = deadline_in(nat(5)).unwrap();
    assert_eq!(remaining(deadline.clone()).unwrap(), nat(5));
    host::advance(2500);
    assert_eq!(remaining(deadline.clone()).unwrap(), nat(2));
    host::advance(10_000);
    assert_eq!(remaining(deadline).unwrap(), nat(0));
    for s in [0u64, 1, 59, 86_400] {
        assert_eq!(to_seconds(to_ticks(nat(s))), nat(s), "the proved round trip");
    }
}

/// TEST0321: the claims about deadlines are conditional on the clock
#[test]
fn test0321_the_claims_about_deadlines_are_conditional_on_the_clock() {
    // Proved, under an assumption the host must make true (the C example runs a host that does
    // not).
    let claim = __meta::assurance().claim("Timing.toSeconds_toTicks").unwrap();
    assert_eq!(claim.status, ClaimStatus::Proved);
    assert_eq!(claim.assumptions, ["Timing.Ticks"]);
    let facility = __meta::assurance().facility("Timing.Clock").unwrap();
    assert_eq!((facility.id, facility.assumptions), ("timing.clock", &["Timing.Ticks"][..]));
    assert_eq!(facility.operations, ["Timing.now", "Timing.ticksPerSecond"]);
    let to_seconds = __meta::declaration("Timing.toSeconds").unwrap();
    assert_eq!(to_seconds.assurance.assumptions, ["Timing.Ticks"]);
    assert_eq!(to_seconds.assurance.facilities, ["Timing.Clock"]);
}
