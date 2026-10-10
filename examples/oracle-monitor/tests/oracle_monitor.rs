use lungo::{ClaimStatus, List};
use oracle_monitor::Index;
use oracle_monitor::access::{self, Effect, Event, Request, Rule, Session};

const ROLES: [&str; 3] = ["admin", "editor", "viewer"];
const ACTIONS: [&str; 3] = ["read", "write", "delete"];

/// A deterministic policy of `n` rules from `seed`.
fn policy(seed: u64, n: usize) -> Vec<Rule> {
    let mut x = seed;
    (0..n)
        .map(|_| {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            Rule {
                role: ROLES[(x >> 40) as usize % 3].into(),
                action: ACTIONS[(x >> 50) as usize % 3].into(),
                effect: if (x >> 33) % 4 == 0 { Effect::Deny } else { Effect::Allow },
            }
        })
        .collect()
}

/// TEST0330: the hand-written evaluator agrees with the proved oracle
#[test]
fn test0330_the_hand_written_evaluator_agrees_with_the_proved_oracle() {
    for seed in 0..300 {
        let rules = policy(seed, (seed % 8) as usize);
        let index = Index::new(&rules);
        let lean = List::from(rules);
        for role in ROLES {
            for action in ACTIONS {
                let oracle = access::evaluate(lean.clone(), Request { role: role.into(), action: action.into() });
                assert_eq!(index.permits(role, action), oracle, "seed {seed}: {role} {action}");
            }
        }
    }
}

/// TEST0331: the monitor reports exactly the protocol's violations
#[test]
fn test0331_the_monitor_reports_exactly_the_protocols_violations() {
    let rules = List::from(vec![
        Rule { role: "editor".into(), action: "write".into(), effect: Effect::Allow },
        Rule { role: "editor".into(), action: "read".into(), effect: Effect::Allow },
    ]);
    let feed = |events: Vec<Event>| -> Result<Session, String> {
        events.into_iter().try_fold(access::start(), |s, e| access::observe(rules.clone(), s, e))
    };
    let sign_in = Event::SignIn { user: "ada".into(), role: "editor".into() };
    let access = |action: &str| Event::Access { user: "ada".into(), action: action.into() };
    assert!(
        feed(vec![sign_in.clone(), access("write"), access("read"), Event::SignOut { user: "ada".into() }]).is_ok()
    );
    assert_eq!(feed(vec![access("read")]), Err("ada is not signed in".into()));
    assert_eq!(feed(vec![sign_in.clone(), access("delete")]), Err("the policy does not permit ada to delete".into()));
    assert_eq!(feed(vec![sign_in.clone(), sign_in]), Err("ada is signed in already".into()));
}

/// TEST0332: the crate reports the oracle and the monitor
#[test]
fn test0332_the_crate_reports_the_oracle_and_the_monitor() {
    let a = access::__meta::assurance();
    let decides = a.claim("Access.evaluate_decides").unwrap();
    assert_eq!((decides.relation, decides.status), ("lungo.decides", ClaimStatus::Proved));
    let sound = a.claim("Access.observe_sound").unwrap();
    assert_eq!((sound.relation, sound.subjects), ("lungo.monitors", &["Access.start", "Access.observe"][..]));
    assert_eq!(sound.specifications, ["Access.Sessions"]);
    let evaluate = access::__meta::declaration("Access.evaluate").unwrap();
    assert_eq!(evaluate.assurance.roles, ["lungo.oracle"]);
}
