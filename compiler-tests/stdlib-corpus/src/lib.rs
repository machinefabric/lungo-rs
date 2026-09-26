//! Lean's executable standard library, compiled through patina.

pub mod corpus {
    include!(concat!(env!("OUT_DIR"), "/patina/stdlib_corpus.rs"));
}
