//! The `Facade` Lean project, generated twice: on the lungo runtime (`pure`) and on Lean's
//! native backend and runtime (`oracle`).

pub mod pure {
    lungo::include_lean!("pure");
}

pub mod oracle {
    lungo::include_lean!("oracle");
}
