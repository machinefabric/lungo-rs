//! A Rust application hosting Lean code that calls back into it.

include!(concat!(env!("OUT_DIR"), "/patina/host.rs"));

/// The Rust implementations of the Lean `@[extern]` declarations in `Host.Callbacks`.
pub mod host {
    use patina::{IoError, LeanClosure, List, Nat};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    static TABLE: Mutex<BTreeMap<String, u64>> = Mutex::new(BTreeMap::new());
    static LOG: Mutex<Vec<String>> = Mutex::new(Vec::new());

    /// Replaces the lookup table.
    pub fn set_table(entries: &[(&str, u64)]) {
        *TABLE.lock().unwrap() = entries.iter().map(|(k, v)| (k.to_string(), *v)).collect();
    }

    /// Takes the lines logged so far.
    pub fn take_log() -> Vec<String> {
        std::mem::take(&mut *LOG.lock().unwrap())
    }

    pub fn lookup(key: String) -> Option<Nat> {
        TABLE.lock().unwrap().get(&key).map(|v| Nat::from(*v as u128))
    }

    pub fn log(line: String) -> Result<(), IoError> {
        if line.is_empty() {
            return Err(IoError::user("empty log line"));
        }
        LOG.lock().unwrap().push(line);
        Ok(())
    }

    pub fn transform(f: LeanClosure<fn(Nat) -> Nat>, xs: List<Nat>) -> List<Nat> {
        xs.into_iter().map(|x| f.call(x)).collect()
    }
}
