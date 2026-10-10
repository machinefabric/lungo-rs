//! An inventory compiled from Lean whose lookups are proved to cost what another package's model
//! allows: the claim `Inventory.lookup_linear` is under Acme's own relation `acme.cost-bound`,
//! about Acme's specification `Acme.Linear` (`acme/`), and lungo carries it as it carries its
//! own.

lungo::include_lean!("inventory");
