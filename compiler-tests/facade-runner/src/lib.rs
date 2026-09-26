//! The `Facade` Lean project, generated twice: on the patina runtime (`pure`) and on Lean's
//! native backend and runtime (`oracle`).

pub mod pure {
    patina::include_lean!("pure");
}

pub mod oracle {
    patina::include_lean!("oracle");
}
