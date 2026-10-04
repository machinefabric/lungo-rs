//! A value made by one program is used by the other: `consumer`'s module takes `provider`'s types.

use host::{consumer, provider};
use lungo::Nat;

/// TEST0006: values pass between programs
#[test]
fn test0006_values_pass_between_programs() {
    let p = provider::mk_pos(Nat::from(3u64)).expect("3 is positive");
    let d = consumer::double(p);
    assert_eq!(provider::value(d.clone()), Nat::from(6u64));
    let q = provider::make_pair(Nat::from(1u64), "apples".to_owned());
    assert_eq!(consumer::describe(consumer::count(d, q)), "apples: 7");
}
