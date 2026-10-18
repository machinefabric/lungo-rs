//! The `Facade` Lean project, generated twice: on the lungo runtime (`pure`) and on Lean's
//! native backend and runtime (`oracle`). There is no oracle on an MSVC target (see `build.rs`),
//! so there neither is generated.

#[cfg(lean_oracle)]
pub mod pure {
    lungo::include_lean!("pure");
}

#[cfg(lean_oracle)]
pub mod oracle {
    lungo::include_lean!("oracle");
}
