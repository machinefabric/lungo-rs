use ledger::{__meta, Event, apply, deposited, replay, withdrawn};
use lungo::{ClaimStatus, List, Nat};

fn nat(n: u64) -> Nat {
    Nat::from(n)
}

fn deposit(n: u64) -> Event {
    Event::Deposit { amount: nat(n) }
}

fn withdraw(n: u64) -> Event {
    Event::Withdraw { amount: nat(n) }
}

/// TEST0323: replay accepts solvent histories and refuses an overdraft
#[test]
fn test0323_replay_accepts_solvent_histories_and_refuses_an_overdraft() {
    let history = List::from(vec![deposit(100), withdraw(30), deposit(5), withdraw(75)]);
    assert_eq!(replay(history.clone()), Some(nat(0)));
    assert_eq!((deposited(history.clone()), withdrawn(history)), (nat(105), nat(105)));
    assert_eq!(replay(List::from(vec![deposit(10), withdraw(11)])), None);
    // An overdraft refused even when a later deposit would cover it: the protocol is about every
    // point of the history.
    assert_eq!(replay(List::from(vec![withdraw(1), deposit(100)])), None);
    assert_eq!(apply(nat(5), withdraw(5)), Some(nat(0)));
    assert_eq!(apply(nat(5), withdraw(6)), None);
}

/// TEST0324: replay agrees with a running balance on generated histories
#[test]
fn test0324_replay_agrees_with_a_running_balance_on_generated_histories() {
    let mut x: u64 = 324;
    for _ in 0..200 {
        let mut events = Vec::new();
        let mut balance: i64 = 0;
        let mut solvent = true;
        for _ in 0..20 {
            x = x.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let amount = (x >> 33) % 50;
            if (x >> 32) & 1 == 0 {
                events.push(deposit(amount));
                balance += amount as i64;
            } else {
                events.push(withdraw(amount));
                balance -= amount as i64;
                solvent &= balance >= 0;
            }
        }
        let expected = solvent.then(|| nat(balance as u64));
        assert_eq!(replay(List::from(events)), expected);
    }
}

/// TEST0325: the crate reports what is proved of replay
#[test]
fn test0325_the_crate_reports_what_is_proved_of_replay() {
    let a = __meta::assurance();
    let solvent = a.claim("Ledger.replay_solvent").unwrap();
    assert_eq!((solvent.status, solvent.specifications), (ClaimStatus::Proved, &["Ledger.Solvent"][..]));
    assert_eq!(a.specification("Ledger.Solvent").unwrap().kind, "lungo.protocol");
    assert_eq!(
        __meta::declaration("Ledger.replay").unwrap().assurance.claims,
        ["Ledger.replay_balance", "Ledger.replay_solvent"]
    );
}
