module

public meta import Lean.Elab.Command
public meta import Lean.Compiler.IR.CompilerM

/-!
The metaprogram generating the corpus: `reference_all_executables n` defines, in chunks of `n`,
arrays holding as a value every constant that has compiled code in an `Init` or `Std` module.
-/

open Lean Meta Elab Command

/-- Constants with compiled runtime code in `Init`/`Std` modules, in name order (`meta`
definitions are compile-time code). -/
public meta def executableConstants (env : Environment) : Array Name := Id.run do
  let mut out := #[]
  for h : i in [0:env.header.moduleNames.size] do
    let m := env.header.moduleNames[i]
    unless (`Init).isPrefixOf m || (`Std).isPrefixOf m do continue
    for d in IR.declMapExt.getModuleIREntries env i do
      if env.contains d.name && !isMarkedMeta env d.name then out := out.push d.name
  return out.qsort Name.lt

public meta def referenceAll (size : Nat) : CommandElabM Unit := do
  let env ← getEnv
  let names := executableConstants env
  let mut start := 0
  let mut index := 0
  while start < names.size do
    let slice := names.extract start (start + size)
    let value ← liftTermElabM do
      let unit := mkConst ``Unit
      let items ← slice.mapM fun n => do
        let some ci := env.find? n | throwError "{n} disappeared"
        let c := mkConst n (ci.levelParams.map fun _ => Level.zero)
        let ty ← inferType c
        let u ← getLevel ty
        return mkApp3 (mkConst ``unsafeCast [u, Level.one]) ty unit c
      instantiateMVars (← mkArrayLit unit items.toList)
    let decl := Declaration.defnDecl {
      name := Name.mkSimple s!"executables{index}", levelParams := [],
      type := mkApp (mkConst ``Array [Level.zero]) (mkConst ``Unit), value,
      hints := .opaque, safety := .unsafe
    }
    liftCoreM (addAndCompile decl)
    start := start + size
    index := index + 1

syntax (name := referenceAllExecutables) "reference_all_executables " num : command

@[command_elab referenceAllExecutables]
public meta def elabReferenceAll : CommandElab
  | `(reference_all_executables $n:num) => referenceAll n.getNat
  | _ => throwUnsupportedSyntax
