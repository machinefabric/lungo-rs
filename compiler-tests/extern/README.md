# The extern-types fixture

Two Lake projects: `provider`, whose `Pos` carries a proof (bindings hold it by handle) and whose
`Pair` is plain data, and `consumer`, which requires `provider` and whose functions take and
return `provider`'s types. `crates/lungo-cli/tests/extern_types.rs` generates `provider` as a
package of its own and `consumer` embedded in a host package that takes `provider`'s types
(`extern-types`), in every language, and runs the programs here against both: values made by one
program pass to the other, and a `provider` generated from another definition of its types is
refused before anything runs.
