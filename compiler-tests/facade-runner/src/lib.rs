//! The `Facade` Lean project, generated twice: on the lean2rust runtime (`pure`) and on Lean's
//! native backend and runtime (`oracle`).

pub mod pure {
    include!(concat!(env!("OUT_DIR"), "/l2r-pure/facade.rs"));
}

pub mod oracle {
    include!(concat!(env!("OUT_DIR"), "/l2r-oracle/facade.rs"));
}
