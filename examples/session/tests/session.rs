use lungo::{List, Nat};
use session::formal::{self, Op, Sess};

fn sess(open: bool, count: u64) -> Sess {
    Sess { is_open: open, count: Nat::from(count) }
}

#[test]
fn apply_follows_the_lean_definition() {
    assert_eq!(formal::apply(Op::Open, sess(false, 0)), Some(sess(true, 0)));
    assert_eq!(formal::apply(Op::Open, sess(true, 3)), None);
    assert_eq!(formal::apply(Op::Close, sess(true, 3)), Some(sess(false, 3)));
    assert_eq!(formal::apply(Op::Close, sess(false, 3)), None);
    assert_eq!(formal::apply(Op::Tick, sess(false, 41)), Some(sess(false, 42)));
}

#[test]
fn counts_beyond_machine_integers_are_exact() {
    let huge: Nat = "340282366920938463463374607431768211455".parse().unwrap();
    let s = Sess { is_open: true, count: huge.clone() };
    let next = formal::apply(Op::Tick, s).unwrap();
    assert_eq!(next.count.to_string(), "340282366920938463463374607431768211456");
    let edge = sess(false, u64::MAX);
    assert_eq!(formal::apply(Op::Tick, edge).unwrap().count.to_string(), "18446744073709551616");
}

#[test]
fn run_stops_at_the_first_rejected_operation() {
    let ops = List::from(vec![Op::Open, Op::Tick, Op::Tick, Op::Close]);
    assert_eq!(formal::run(ops, sess(false, 0)), Some(sess(false, 2)));
    let bad = List::from(vec![Op::Close, Op::Tick]);
    assert_eq!(formal::run(bad, sess(false, 0)), None);
}

#[test]
fn metadata_identifies_the_lean_declaration() {
    let info = formal::__meta::declaration("Formal.apply").expect("Formal.apply is exported");
    assert_eq!(info.module, "Formal.Session");
    assert_eq!(info.lean_type, "Formal.Op → Formal.Sess → Option Formal.Sess");
    assert_eq!(info.source_file, Some("lean/Formal/Session.lean"));
    assert_eq!(info.rust_path, "apply");
    assert!(!info.trust.depends_on_sorry);
    assert!(formal::__meta::declaration("Formal.apply_preserves_inv").is_none());
}
