//! What every backend shares: extern resolution, naming, the source writer, and error codes.

pub mod codes;
pub mod exports;
pub mod externs;
pub mod interface;
pub mod model;
pub mod names;
pub mod naming;
pub mod writer;
