use lungo::{List, Nat};
use session::formal::{self, Op, Sess};

fn sess(open: bool, count: u64) -> Sess {
    Sess { is_open: open, count: Nat::from(count) }
}

/// TEST0256: apply follows the lean definition
#[test]
fn test0256_apply_follows_the_lean_definition() {
    assert_eq!(formal::apply(Op::Open, sess(false, 0)), Some(sess(true, 0)));
    assert_eq!(formal::apply(Op::Open, sess(true, 3)), None);
    assert_eq!(formal::apply(Op::Close, sess(true, 3)), Some(sess(false, 3)));
    assert_eq!(formal::apply(Op::Close, sess(false, 3)), None);
    assert_eq!(formal::apply(Op::Tick, sess(false, 41)), Some(sess(false, 42)));
}

/// TEST0257: counts beyond machine integers are exact
#[test]
fn test0257_counts_beyond_machine_integers_are_exact() {
    let huge: Nat = "340282366920938463463374607431768211455".parse().unwrap();
    let s = Sess { is_open: true, count: huge.clone() };
    let next = formal::apply(Op::Tick, s).unwrap();
    assert_eq!(next.count.to_string(), "340282366920938463463374607431768211456");
    let edge = sess(false, u64::MAX);
    assert_eq!(formal::apply(Op::Tick, edge).unwrap().count.to_string(), "18446744073709551616");
}

/// TEST0258: run stops at the first rejected operation
#[test]
fn test0258_run_stops_at_the_first_rejected_operation() {
    let ops = List::from(vec![Op::Open, Op::Tick, Op::Tick, Op::Close]);
    assert_eq!(formal::run(ops, sess(false, 0)), Some(sess(false, 2)));
    let bad = List::from(vec![Op::Close, Op::Tick]);
    assert_eq!(formal::run(bad, sess(false, 0)), None);
}

/// TEST0259: metadata identifies the lean declaration
#[test]
fn test0259_metadata_identifies_the_lean_declaration() {
    let info = formal::__meta::declaration("Formal.apply").expect("Formal.apply is exported");
    assert_eq!(info.module, "Formal.Session");
    assert_eq!(info.lean_type, "Formal.Op → Formal.Sess → Option Formal.Sess");
    assert_eq!(info.source_file, Some("lean/Formal/Session.lean"));
    assert_eq!(info.rust_path, "apply");
    assert!(!info.trust.depends_on_sorry);
    assert!(formal::__meta::declaration("Formal.step_implements").is_none(), "a theorem is not exported");
}

/// TEST0307: step is proved to do what the specification says
#[test]
fn test0307_step_is_proved_to_do_what_the_specification_says() {
    assert_eq!(formal::step(sess(false, 4), Op::Open), (true, sess(true, 4)));
    assert_eq!(formal::step(sess(true, 4), Op::Open), (false, sess(true, 4)));
    assert_eq!(formal::step(sess(true, 4), Op::Tick), (true, sess(true, 5)));
    let assurance = formal::__meta::assurance();
    let claim = assurance.claim("Formal.step_implements").expect("the claim is reported");
    assert_eq!((claim.relation, claim.status), ("lungo.preserves", lungo::ClaimStatus::Proved));
    assert_eq!((claim.subjects, claim.specifications), (&["Formal.step"][..], &["Formal.Spec"][..]));
    assert!(claim.assumptions.is_empty(), "the session needs nothing of the host");
    assert_eq!(assurance.specification("Formal.Spec").map(|s| s.kind), Some("lungo.state"));
    let step = formal::__meta::declaration("Formal.step").unwrap();
    assert_eq!(step.assurance.claims, ["Formal.step_implements"]);
    let run = formal::__meta::declaration("Formal.run").unwrap();
    assert_eq!(run.assurance.claims, ["Formal.run_ticks"]);
    assert_eq!(formal::__meta::declaration("Formal.apply").unwrap().assurance.claims, [] as [&str; 0]);
}
