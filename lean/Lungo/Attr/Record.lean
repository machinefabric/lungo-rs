module

public meta import Lean
public import Lungo.Registry
public meta import Lungo.Attr.Names

/-!
# Writing records

Records are values in one canonical form — exactly the constructor applications built here, with
`Name`s spelled with `Name.str` and `Name.num` and literal strings — so lungo's worker can read
them without evaluating anything, and refuses any other form.

A record is added as a definition with no compiled code (it is `noncomputable`), exposed so the
worker can read its value from any module that imports it. Only the module declaring a
declaration may register it: a record must be read where its declaration is.
-/

@[expose] public section

namespace Lungo.Attr

open Lean

meta def nameExpr : Name → Expr
  | .anonymous => mkConst ``Name.anonymous
  | .str p s => mkApp2 (mkConst ``Name.str) (nameExpr p) (mkStrLit s)
  | .num p n => mkApp2 (mkConst ``Name.num) (nameExpr p) (mkRawNatLit n)

meta def namesExpr (xs : List Name) : Expr :=
  xs.foldr (fun x acc => mkApp3 (mkConst ``List.cons [Level.zero]) (mkConst ``Name) (nameExpr x) acc)
    (mkApp (mkConst ``List.nil [Level.zero]) (mkConst ``Name))

meta def optionNameExpr : Option Name → Expr
  | none => mkApp (mkConst ``Option.none [Level.zero]) (mkConst ``Name)
  | some n => mkApp2 (mkConst ``Option.some [Level.zero]) (mkConst ``Name) (nameExpr n)

/-- Fails unless `decl` is declared by the module being elaborated. -/
meta def ensureLocal (attr : String) (decl : Name) : CoreM Unit := do
  if let some idx := (← getEnv).getModuleIdxFor? decl then
    let mod := (← getEnv).header.moduleNames[idx.toNat]!
    throwError "`@[{attr}]` must be given where `{decl}` is declared (module `{mod}`): lungo reads \
      a declaration's records from the module that declares it"

/-- Adds the record `decl._lungo_<kind> : structName := value`. -/
meta def addRecord (attr : String) (decl : Name) (kind : String) (structName : Name)
    (value : Expr) : CoreM Unit := do
  ensureLocal attr decl
  let name := recordName decl kind
  if (← getEnv).contains name then
    throwError "`{decl}` already has `@[{attr}]`: a declaration is registered once"
  addDecl (.defnDecl {
    name, levelParams := [], type := mkConst structName, value
    hints := .opaque, safety := .safe, all := [name]
  }) (forceExpose := true)
  modifyEnv (addNoncomputable · name)

/-- The value of the record `decl._lungo_<kind>`, if `decl` has one. -/
meta def record? (decl : Name) (kind : String) : CoreM (Option Expr) := do
  match (← getEnv).find? (recordName decl kind) with
  | some (.defnInfo d) => return some d.value
  | _ => return none

end Lungo.Attr
