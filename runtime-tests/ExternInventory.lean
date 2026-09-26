import Lean
open Lean Lean.IR

/-!
Inventories the native interface of the Lean toolchain's `Init` and `Std` libraries, with the
C-level signatures Lean's code generator uses: erased and `void` parameters are dropped, object
parameters are marked borrowed (`b_obj`) or owned (`obj`).

Run with the toolchain the bridge supports:

    lean --run runtime-tests/ExternInventory.lean externs > crates/lungo-runtime/tests/data/toolchain-externs.txt
    lean --run runtime-tests/ExternInventory.lean exports > crates/lungo-runtime/tests/data/toolchain-exports.txt

`externs` lists every `@[extern]` compiler declaration, one per line:
`symbol<TAB>declaration<TAB>param,param,...<TAB>result<TAB>implementation`, where the
implementation is the Lean definition that `@[export]`s the symbol, or `-` when the symbol must be
provided natively.

`exports` lists every `@[export]`ed compiled definition, one per line:
`symbol<TAB>declaration<TAB>param,param,...<TAB>result`.
-/

def ty (t : IRType) (borrow : Bool) : String :=
  match t with
  | .float => "f64"
  | .float32 => "f32"
  | .uint8 => "u8"
  | .uint16 => "u16"
  | .uint32 => "u32"
  | .uint64 => "u64"
  | .usize => "usize"
  | .object | .tobject | .tagged => if borrow then "b_obj" else "obj"
  -- An erased or `void` result is still returned as an object (`lean_box(0)`).
  | .erased | .void => "obj"
  | .struct .. | .union .. => panic! "IR struct/union types are not produced by this toolchain"

def signature (xs : Array Param) (t : IRType) : String :=
  let ps := xs.filter (fun p => !(p.ty matches .erased | .void))
  s!"{",".intercalate (ps.map (fun p => ty p.ty p.borrow)).toList}\t{ty t false}"

def main (args : List String) : IO Unit := do
  let mode ← match args with
    | [m@"externs"] | [m@"exports"] => pure m
    | _ => throw (IO.userError "usage: lean --run ExternInventory.lean (externs | exports)")
  initSearchPath (← findSysroot)
  let env ← importModules #[{ module := `Init }, { module := `Std }] {} (level := .private)
  let mut exported : Std.HashMap String Name := {}
  for i in [0:env.header.moduleNames.size] do
    for (decl, sym) in exportAttr.ext.getModuleEntries env i do
      exported := exported.insert (sym.toString (escape := false)) decl
  let mut lines : Array String := #[]
  if mode == "externs" then
    for i in [0:env.header.moduleNames.size] do
      for d in declMapExt.getModuleIREntries env i do
        if let .extern f xs t data := d then
          let some (.standard _ sym) := getExternEntryForAux `c data.entries | continue
          let impl := match exported[sym]? with
            | some g => if g != f then g.toString else "-"
            | none => "-"
          lines := lines.push s!"{sym}\t{f}\t{signature xs t}\t{impl}"
  else
    for (sym, decl) in exported.toList do
      let some d := findEnvDecl env decl | continue
      lines := lines.push s!"{sym}\t{decl}\t{signature d.params d.resultType}"
  for l in lines.qsort (· < ·) do
    IO.println l
