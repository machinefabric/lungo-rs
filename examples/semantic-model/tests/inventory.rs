use lungo::{ClaimStatus, List, Nat};
use semantic_model::{__meta, Item, lookup, lookup_steps};

fn items(n: u64) -> List<Item> {
    (0..n).map(|i| Item { sku: format!("sku-{i}"), count: Nat::from(i * 10) }).collect()
}

/// TEST0341: lookups find counts within the cost the model allows
#[test]
fn test0341_lookups_find_counts_within_the_cost_the_model_allows() {
    let inventory = items(50);
    assert_eq!(lookup("sku-7".into(), inventory.clone()), Some(Nat::from(70u64)));
    assert_eq!(lookup("nothing".into(), inventory.clone()), None);
    for (sku, steps) in [("sku-0", 1u64), ("sku-49", 50), ("nothing", 51)] {
        let taken = lookup_steps(sku.into(), inventory.clone());
        assert_eq!(taken, Nat::from(steps), "{sku}");
        assert!(taken <= Nat::from(50u64 + 1), "Acme.Linear List.length 1");
    }
}

/// TEST0342: a claim under another package's relation is reported as lungo's own are
#[test]
fn test0342_a_claim_under_another_packages_relation_is_reported_as_lungos_own_are() {
    let a = __meta::assurance();
    let claim = a.claim("Inventory.lookup_linear").unwrap();
    assert_eq!((claim.relation, claim.status), ("acme.cost-bound", ClaimStatus::Proved));
    assert_eq!((claim.subjects, claim.specifications), (&["Inventory.lookupSteps"][..], &["Acme.Linear"][..]));
    let spec = a.specification("Acme.Linear").unwrap();
    assert_eq!((spec.kind, spec.package), ("acme.cost-model", Some("acme")));
    assert_eq!(a.claim("Inventory.lookup_eq").unwrap().package, Some("inventory"));
}
