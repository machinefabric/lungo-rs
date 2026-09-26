//! The `Facade` Lean project, generated twice: on the patina runtime (`pure`) and on Lean's
//! native backend and runtime (`oracle`).

pub mod pure {
    include!(concat!(env!("OUT_DIR"), "/ptn-pure/facade.rs"));
}

pub mod oracle {
    include!(concat!(env!("OUT_DIR"), "/ptn-oracle/facade.rs"));
}
