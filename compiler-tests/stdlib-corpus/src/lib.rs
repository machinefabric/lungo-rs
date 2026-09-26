//! Lean's executable standard library, compiled through patina.

pub mod corpus {
    patina::include_lean!("stdlib");
}
