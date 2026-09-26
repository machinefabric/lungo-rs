//! Lean's executable standard library, compiled through lean2rust.

pub mod corpus {
    include!(concat!(env!("OUT_DIR"), "/lean2rust/stdlib_corpus.rs"));
}
