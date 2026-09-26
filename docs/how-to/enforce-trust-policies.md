# How to reject exports that rely on `sorry`, axioms or unsafe code

This guide shows how to make the build fail when an exported Lean declaration depends on
unproven or unchecked code, and how to audit what exports depend on.

## Choose the policies

Three policies apply to every exported declaration:

| Policy | Default | Fails the build when an export… |
| --- | --- | --- |
| `deny_sorry` | on | depends on `sorry` |
| `deny_axioms` | off | depends on an axiom beyond `propext`, `Classical.choice`, `Quot.sound` |
| `deny_unsafe` | off | runs `unsafe` code outside the Lean toolchain |

Enable the ones you need in `build.rs`:

```rust
fn main() -> lungo_build::Result<()> {
    lungo_build::configure().deny_axioms(true).deny_unsafe(true).compile_lean("lean")
}
```

A violation fails with [`LNG0601`](../reference/errors.md#lng0601) and lists each export with
the offending dependencies:

```text
error[LNG0601]: exported declarations violate the configured trust policy:
  Formal.step depends on `sorry` (deny_sorry)
```

Remove the dependency in Lean, or stop exporting the declaration.

`deny_sorry` is on by default; turn it off only for work in progress
(`.deny_sorry(false)`).

## Audit without failing

To see the dependencies of every export without enforcing a policy, read `manifest.json` in
the build output (the `trust` of each export), inspect one declaration with
`cargo lungo inspect <name>`, or read the metadata at run time:

```rust
for d in formal::__meta::declarations() {
    println!("{}: axioms {:?}, partial {:?}", d.lean_name, d.trust.axioms, d.trust.partial_dependencies);
}
```

`partial` definitions and extern dependencies are reported but not governed by a policy: they
are ordinary parts of executable Lean code.
