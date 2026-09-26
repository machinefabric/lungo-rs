module

public import Lean
public import Lean.Compiler.LCNF.Main

namespace Lean2Rust.BridgeIR

open Lean Lean.Compiler.LCNF

public abbrev version : Nat := 1

private def obj (tag : String) (fields : List (String × Json)) : Json :=
  Json.mkObj (("op", toJson tag) :: fields)

private def num (n : Nat) : Json := toJson n.repr

private def name (n : Name) : Json := toJson n.toString

private def var (v : FVarId) : Json := name v.name

private def binderInfo : BinderInfo → Json
  | .default => toJson "explicit"
  | .implicit => toJson "implicit"
  | .strictImplicit => toJson "strict_implicit"
  | .instImplicit => toJson "instance"

private partial def level : Level → Json
  | .zero => obj "zero" []
  | .succ x => obj "successor" [("value", level x)]
  | .max a b => obj "maximum" [("left", level a), ("right", level b)]
  | .imax a b => obj "dependent_maximum" [("left", level a), ("right", level b)]
  | .param n => obj "parameter" [("name", name n)]
  | .mvar id => obj "metavariable" [("id", name id.name)]

private partial def type : Expr → Json
  | .bvar n => obj "bound_variable" [("index", num n)]
  | .fvar id => obj "free_variable" [("id", var id)]
  | .mvar id => obj "metavariable" [("id", name id.name)]
  | .sort u => obj "sort" [("level", level u)]
  | .const n [] => name n
  | .const n us => obj "constant" [("name", name n), ("levels", toJson (us.map level))]
  | .app fn arg => obj "application" [("function", type fn), ("argument", type arg)]
  | .lam n domain body info => obj "lambda" [
      ("name", name n), ("domain", type domain), ("body", type body),
      ("binder", binderInfo info)]
  | .forallE n domain body info => obj "forall" [
      ("name", name n), ("domain", type domain), ("body", type body),
      ("binder", binderInfo info)]
  | .letE n ty value body nondep => obj "let" [
      ("name", name n), ("type", type ty), ("value", type value),
      ("body", type body), ("nondependent", toJson nondep)]
  | .lit (.natVal n) => obj "natural_literal" [("value", num n)]
  | .lit (.strVal s) => obj "string_literal" [("value", toJson s)]
  | .mdata _ e => type e
  | .proj n i e => obj "projection" [
      ("typeName", name n), ("index", num i), ("value", type e)]

private def arg : Arg .impure → Json
  | .erased => obj "erased" []
  | .fvar v => obj "var" [("id", var v)]

private def args (xs : Array (Arg .impure)) : Json :=
  toJson (xs.toList.map arg)

private def ctorInfo (info : CtorInfo) : Json :=
  Json.mkObj [
    ("name", name info.name),
    ("tag", num info.cidx),
    ("objectFields", num info.size),
    ("usizeFields", num info.usize),
    ("scalarBytes", num info.ssize)
  ]

private def literal : LitValue → Json
  | .nat n => obj "nat" [("value", num n)]
  | .str s => obj "string" [("value", toJson s)]
  | .uint8 n => obj "uint8" [("value", num n.toNat)]
  | .uint16 n => obj "uint16" [("value", num n.toNat)]
  | .uint32 n => obj "uint32" [("value", num n.toNat)]
  | .uint64 n => obj "uint64" [("value", num n.toNat)]
  | .usize n => obj "usize" [("value", num n.toNat)]

private def value : LetValue .impure → Json
  | .lit x => obj "literal" [("literal", literal x)]
  | .erased => obj "erased" []
  | .fvar f xs => obj "apply_closure" [("function", var f), ("args", args xs)]
  | .ctor info xs => obj "constructor" [("info", ctorInfo info), ("args", args xs)]
  | .oproj i v => obj "object_projection" [("index", num i), ("value", var v)]
  | .uproj i v => obj "usize_projection" [("index", num i), ("value", var v)]
  | .sproj n offset v => obj "scalar_projection" [("bytes", num n), ("offset", num offset), ("value", var v)]
  | .fap fn xs => obj "call" [("function", name fn), ("args", args xs)]
  | .pap fn xs => obj "partial_application" [("function", name fn), ("args", args xs)]
  | .reset n v => obj "reset" [("fields", num n), ("value", var v)]
  | .reuse v info updateHeader xs => obj "reuse" [
      ("value", var v), ("info", ctorInfo info), ("updateHeader", toJson updateHeader),
      ("args", args xs)]
  | .box ty v => obj "box" [("type", type ty), ("value", var v)]
  | .unbox v => obj "unbox" [("value", var v)]
  | .isShared v => obj "is_shared" [("value", var v)]

private def param (p : Param .impure) : Json := Json.mkObj [
  ("id", var p.fvarId),
  ("type", type p.type),
  ("borrow", toJson p.borrow)
]

private partial def code : Code .impure → Json
  | .let d k => obj "let" [
      ("id", var d.fvarId), ("type", type d.type), ("value", value d.value),
      ("next", code k)]
  | .jp (.mk id _ ps ty body) next => obj "join" [
      ("id", var id), ("params", toJson (ps.toList.map param)),
      ("type", type ty), ("body", code body), ("next", code next)]
  | .jmp id xs => obj "jump" [("target", var id), ("args", args xs)]
  | .cases (.mk ty result discr alts) => obj "cases" [
      ("typeName", name ty), ("resultType", type result),
      ("discriminator", var discr), ("alternatives", toJson (alts.toList.map alt))]
  | .return id => obj "return" [("value", var id)]
  | .unreach ty => obj "unreachable" [("type", type ty)]
  | .oset v i y k => obj "object_set" [("value", var v), ("index", num i), ("field", arg y), ("next", code k)]
  | .uset v i y k => obj "usize_set" [("value", var v), ("index", num i), ("field", var y), ("next", code k)]
  | .sset v i offset y ty k => obj "scalar_set" [
      ("value", var v), ("index", num i), ("offset", num offset),
      ("field", var y), ("type", type ty), ("next", code k)]
  | .setTag v tag k => obj "set_tag" [("value", var v), ("tag", num tag), ("next", code k)]
  | .inc v n check persistent k => obj "increment" [
      ("value", var v), ("count", num n), ("check", toJson check),
      ("persistent", toJson persistent), ("next", code k)]
  | .dec v n check persistent objs k => obj "decrement" [
      ("value", var v), ("count", num n), ("check", toJson check),
      ("persistent", toJson persistent), ("objects", toJson (objs.map num)), ("next", code k)]
  | .del v k => obj "delete" [("value", var v), ("next", code k)]
where
  alt : Alt .impure → Json
    | .ctorAlt info body => obj "constructor" [("info", ctorInfo info), ("body", code body)]
    | .default body => obj "default" [("body", code body)]

private def externEntry : ExternEntry → Json
  | .adhoc backend => obj "adhoc" [("backend", name backend)]
  | .inline backend pattern => obj "inline" [("backend", name backend), ("pattern", toJson pattern)]
  | .standard backend symbol => obj "standard" [("backend", name backend), ("symbol", toJson symbol)]
  | .opaque => obj "opaque" []

public def declaration (decl : Decl .impure) : Json :=
  Json.mkObj [
    ("name", name decl.name),
    ("params", toJson (decl.params.toList.map param)),
    ("resultType", type decl.type),
    ("safe", toJson decl.safe),
    ("recursive", toJson decl.recursive),
    ("value", match decl.value with
      | .code c => obj "code" [("body", code c)]
      | .extern data => obj "extern" [("entries", toJson (data.entries.map externEntry))])
  ]

end Lean2Rust.BridgeIR
