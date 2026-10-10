//! An account as a history of events, compiled from Lean. Every history [`replay`] accepts is
//! proved solvent (`Ledger.replay_solvent`: nothing is ever withdrawn that was not deposited), and
//! the balance it reports is proved to be what was deposited less what was withdrawn
//! (`Ledger.replay_balance`).

lungo::include_lean!("ledger");
