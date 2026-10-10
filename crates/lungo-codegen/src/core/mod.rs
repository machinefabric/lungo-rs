//! What every backend shares: extern resolution, naming, layout fingerprints, the source writer,
//! and error codes.

pub mod assurance;
pub mod codes;
pub mod exports;
pub mod externs;
pub mod fingerprint;
pub mod interface;
pub mod model;
pub mod names;
pub mod naming;
pub mod writer;
