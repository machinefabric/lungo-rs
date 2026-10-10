//! Access control specified in Lean, used two ways from Rust.
//!
//! [`access::evaluate`] is proved to decide `Access.Permits` exactly, and is used as a test
//! oracle: [`Index`], an evaluator written by hand for speed, is tested against it rather than
//! trusted on its own. [`access::observe`] is a runtime monitor proved to report exactly the
//! violations of the session protocol.

pub mod access {
    lungo::include_lean!("access");
}

use access::{Effect, Rule};
use std::collections::HashMap;

/// A policy indexed by role and action: which effects its rules give each pair. Written by hand,
/// and tested against the proved [`access::evaluate`].
pub struct Index {
    effects: HashMap<(String, String), (bool, bool)>,
}

impl Index {
    pub fn new(policy: &[Rule]) -> Index {
        let mut effects: HashMap<(String, String), (bool, bool)> = HashMap::new();
        for rule in policy {
            let e = effects.entry((rule.role.clone(), rule.action.clone())).or_default();
            match rule.effect {
                Effect::Allow => e.0 = true,
                Effect::Deny => e.1 = true,
            }
        }
        Index { effects }
    }

    /// Whether the policy permits `role` to perform `action`: an allowing rule and no denying one.
    pub fn permits(&self, role: &str, action: &str) -> bool {
        self.effects.get(&(role.to_owned(), action.to_owned())).is_some_and(|&(allow, deny)| allow && !deny)
    }
}
