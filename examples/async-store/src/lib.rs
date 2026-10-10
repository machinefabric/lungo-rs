//! Async programs over a key-value store the host keeps, compiled from Lean: each export is an
//! `async fn` taking a [`StoreOpHandler`] that performs the program's operations. What `copy` and
//! `swap` do to a store is proved against a model of one (`Store.copy_model`, `Store.swap_model`).

lungo::include_lean!("store");
