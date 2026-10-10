use lungo::{ClaimStatus, List, Nat};
use sort::{__meta, Entry, rank, sort};

fn nats(xs: &[u64]) -> List<Nat> {
    xs.iter().map(|&x| Nat::from(x)).collect()
}

/// A deterministic sequence of pseudo-random numbers below `bound`.
fn numbers(seed: u64, n: usize, bound: u64) -> Vec<u64> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x % bound
        })
        .collect()
}

/// TEST0308: sort agrees with the standard library's sort
#[test]
fn test0308_sort_agrees_with_the_standard_librarys_sort() {
    assert_eq!(sort(nats(&[])), nats(&[]));
    assert_eq!(sort(nats(&[3, 1, 2, 1])), nats(&[1, 1, 2, 3]));
    for seed in 1..50 {
        let mut xs = numbers(seed, seed as usize * 7, 1000);
        let sorted = sort(nats(&xs));
        xs.sort();
        assert_eq!(sorted, nats(&xs), "seed {seed}");
    }
    // Beyond machine integers.
    let big: Nat = "100000000000000000000000000000".parse().unwrap();
    assert_eq!(sort(List::from(vec![big.clone(), Nat::from(1u64)])), List::from(vec![Nat::from(1u64), big]));
}

/// TEST0309: rank orders by score and keeps ties in order
#[test]
fn test0309_rank_orders_by_score_and_keeps_ties_in_order() {
    let entry = |name: &str, score: u64| Entry { name: name.into(), score: Nat::from(score) };
    let ranked = rank(List::from(vec![entry("ada", 3), entry("bo", 9), entry("cy", 3), entry("di", 9)]));
    let names: Vec<String> = ranked.into_iter().map(|e| e.name).collect();
    assert_eq!(names, ["bo", "di", "ada", "cy"]);
    for seed in 1..30 {
        let scores = numbers(seed, 40, 5);
        let entries: Vec<Entry> = scores.iter().enumerate().map(|(i, &s)| entry(&format!("e{i}"), s)).collect();
        let mut expected = entries.clone();
        expected.sort_by(|a, b| b.score.cmp(&a.score)); // std's sort is stable too
        assert_eq!(rank(List::from(entries)), List::from(expected), "seed {seed}");
    }
}

/// TEST0310: the crate reports what is proved of its sorts
#[test]
fn test0310_the_crate_reports_what_is_proved_of_its_sorts() {
    let a = __meta::assurance();
    for (claim, subject, spec) in [
        ("Sorting.sort_satisfies", "Sorting.sort", "Sorting.SortsAscending"),
        ("Sorting.rank_satisfies", "Sorting.rank", "Sorting.RanksByScore"),
    ] {
        let c = a.claim(claim).unwrap();
        assert_eq!((c.relation, c.status), ("lungo.satisfies", ClaimStatus::Proved));
        assert_eq!((c.subjects, c.specifications), (&[subject][..], &[spec][..]));
        assert!(
            c.assumptions.is_empty()
                && c.evidence_axioms.iter().all(|x| ["propext", "Classical.choice", "Quot.sound"].contains(x))
        );
    }
    assert_eq!(a.specification("Sorting.SortsAscending").unwrap().kind, "lungo.contract");
    let rank = __meta::declaration("Sorting.rank").unwrap();
    assert_eq!(rank.assurance.claims, ["Sorting.rank_satisfies", "Sorting.rank_stable"]);
}
