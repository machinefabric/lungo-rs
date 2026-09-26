import Lean
import Patina.Cbor
import Patina.Diagnostics

/-!
Serialization of Lean's final compiler representation as patina Bridge IR (BIR).

For Lean 4.34.1 the final representation of every compiled declaration — local or imported — is
the lowering of its final impure LCNF (`Lean.IR.ToIR`), which Lean persists per module and which
its interpreter executes. BIR mirrors that representation instruction for instruction, but
flattens straight-line code into blocks so that consumers do not recurse once per statement.
-/
namespace Patina.BridgeIR

open Lean Lean.IR Patina.Cbor

/-- Version of the BIR data model, independent of the protocol version. -/
def version : Nat := 3

/-- Fully qualified Lean names are canonical identities; the escaped form round-trips. -/
def nameString (n : Name) : String := n.toString

def name (n : Name) : Value := str (nameString n)

def irType (ty : IRType) : Except String Value :=
  match ty with
  | .float => return unitVariant "float"
  | .float32 => return unitVariant "float32"
  | .uint8 => return unitVariant "uint8"
  | .uint16 => return unitVariant "uint16"
  | .uint32 => return unitVariant "uint32"
  | .uint64 => return unitVariant "uint64"
  | .usize => return unitVariant "usize"
  | .erased => return unitVariant "erased"
  | .object => return unitVariant "object"
  | .tobject => return unitVariant "tobject"
  | .tagged => return unitVariant "tagged"
  | .void => return unitVariant "void"
  | .struct .. | .union .. =>
    throw "IR struct/union types are not produced by the Lean 4.34.1 compiler and are rejected by this adapter"

def arg : IR.Arg → Value
  | .var x => .map #[("var", nat x.idx)]
  | .erased => unitVariant "erased"

def args (ys : Array IR.Arg) : Value := arr (ys.map arg)

def ctorInfo (i : IR.CtorInfo) : Value :=
  obj [("name", name i.name), ("tag", nat i.cidx), ("size", nat i.size),
       ("usize", nat i.usize), ("ssize", nat i.ssize)]

def param (p : IR.Param) : Except String Value := do
  return obj [("var", nat p.x.idx), ("ty", ← irType p.ty), ("borrow", .bool p.borrow)]

def expr : IR.Expr → Except String Value
  | .ctor i ys => return variant "ctor" [("info", ctorInfo i), ("args", args ys)]
  | .reset n x => return variant "reset" [("fields", nat n), ("var", nat x.idx)]
  | .reuse x i u ys =>
    return variant "reuse" [("var", nat x.idx), ("info", ctorInfo i), ("update_header", .bool u), ("args", args ys)]
  | .proj i x => return variant "proj" [("index", nat i), ("var", nat x.idx)]
  | .uproj i x => return variant "uproj" [("index", nat i), ("var", nat x.idx)]
  | .sproj n o x => return variant "sproj" [("fields", nat n), ("offset", nat o), ("var", nat x.idx)]
  | .fap c ys => return variant "fap" [("function", name c), ("args", args ys)]
  | .pap c ys => return variant "pap" [("function", name c), ("args", args ys)]
  | .ap x ys => return variant "ap" [("var", nat x.idx), ("args", args ys)]
  | .box ty x => return variant "box" [("ty", ← irType ty), ("var", nat x.idx)]
  | .unbox x => return variant "unbox" [("var", nat x.idx)]
  | .lit (.num n) => return .map #[("lit", .map #[("num", str (toString n))])]
  | .lit (.str s) => return .map #[("lit", .map #[("str", str s)])]
  | .isShared x => return variant "is_shared" [("var", nat x.idx)]

mutual

partial def block (b : IR.FnBody) : Except String Value := do
  let mut stmts : Array Value := #[]
  let mut b := b
  repeat
    match b with
    | .vdecl x ty e k =>
      stmts := stmts.push (variant "let" [("var", nat x.idx), ("ty", ← irType ty), ("expr", ← expr e)])
      b := k
    | .jdecl j xs v k =>
      stmts := stmts.push (variant "join"
        [("id", nat j.idx), ("params", arr (← xs.mapM param)), ("body", ← block v)])
      b := k
    | .set x i y k =>
      stmts := stmts.push (variant "set" [("var", nat x.idx), ("index", nat i), ("arg", arg y)])
      b := k
    | .setTag x c k =>
      stmts := stmts.push (variant "set_tag" [("var", nat x.idx), ("tag", nat c)])
      b := k
    | .uset x i y k =>
      stmts := stmts.push (variant "uset" [("var", nat x.idx), ("index", nat i), ("value", nat y.idx)])
      b := k
    | .sset x i o y ty k =>
      stmts := stmts.push (variant "sset" [("var", nat x.idx), ("index", nat i), ("offset", nat o),
        ("value", nat y.idx), ("ty", ← irType ty)])
      b := k
    | .inc x n c p k =>
      stmts := stmts.push (variant "inc" [("var", nat x.idx), ("count", nat n), ("checked", .bool c),
        ("persistent", .bool p)])
      b := k
    | .dec x n c p k =>
      stmts := stmts.push (variant "dec" [("var", nat x.idx), ("count", nat n), ("checked", .bool c),
        ("persistent", .bool p)])
      b := k
    | .del x k =>
      stmts := stmts.push (variant "del" [("var", nat x.idx)])
      b := k
    | _ => break
  return obj [("stmts", arr stmts), ("terminator", ← terminator b)]

partial def terminator : IR.FnBody → Except String Value
  | .case tid x xty alts => do
    return variant "case" [("type_name", name tid), ("var", nat x.idx), ("var_ty", ← irType xty),
      ("alts", arr (← alts.mapM alt))]
  | .ret y => return variant "ret" [("arg", arg y)]
  | .jmp j ys => return variant "jmp" [("id", nat j.idx), ("args", args ys)]
  | .unreachable => return unitVariant "unreachable"
  | _ => throw "internal error: non-terminal IR instruction in terminator position"

partial def alt : IR.Alt → Except String Value
  | .ctor info b => return variant "ctor" [("info", ctorInfo info), ("body", ← block b)]
  | .default b => return variant "default" [("body", ← block b)]

end

def externEntry : ExternEntry → Value
  | .adhoc backend => variant "adhoc" [("backend", name backend)]
  | .inline backend pattern => variant "inline" [("backend", name backend), ("pattern", str pattern)]
  | .standard backend symbol => variant "standard" [("backend", name backend), ("symbol", str symbol)]
  | .opaque => unitVariant "opaque"

end Patina.BridgeIR
