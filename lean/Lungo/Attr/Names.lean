module

public meta import Lean

/-!
# Kinds, roles and record names

A specification kind, a claim's relation, a role and a capability's identifier are namespaced
strings: two or more `.`-separated segments of lowercase letters, digits, `_` (and `-` after the
first segment), each starting with a letter — `lungo.decides`, `acme.cost-bound`, `time.clock`.
The `lungo` namespace is lungo's own: only the kinds listed here exist in it. Any other namespace
belongs to whoever defines it, and lungo carries its kinds without interpreting them.
-/

@[expose] public section

namespace Lungo.Attr

open Lean

/-- Relations whose evidence lungo knows the shape of, and checks. -/
meta def claimRelations : List String :=
  ["decides", "satisfies", "refines", "preserves", "equals", "roundtrip", "law", "monitors"]

/-- Specification kinds of lungo's own. -/
meta def specKinds : List String :=
  ["relation", "contract", "model", "state", "protocol", "property"]

/-- Roles of lungo's own. -/
meta def roles : List String :=
  ["implementation", "oracle", "monitor", "model"]

private meta def segmentOk (first : Bool) (s : String) : Bool :=
  match s.toList with
  | [] => false
  | c :: cs =>
    c.isLower && cs.all fun d => d.isLower || d.isDigit || d == '_' || (!first && d == '-')

/-- Whether `s` is a well-formed namespaced kind. -/
meta def wellFormed (s : String) : Bool :=
  match s.splitOn "." with
  | [] | [_] => false
  | first :: rest => segmentOk true first && rest.all (segmentOk false)

/-- Checks the namespaced string `s` used as `what`; in the `lungo` namespace, it must be one of
`known`. -/
meta def checkKind (what : String) (s : String) (known : List String) (sample : String) :
    CoreM Unit := do
  unless wellFormed s do
    throwError "`{s}` is not a valid {what}: it must be two or more `.`-separated segments of \
      lowercase letters, digits and `_` (and `-` after the first), each starting with a letter, \
      such as `{sample}`"
  if let "lungo" :: rest := s.splitOn "." then
    let name := ".".intercalate rest
    unless known.contains name do
      if known.isEmpty then
        throwError "`{s}`: the `lungo` namespace is lungo's own, and holds no {what}; use a \
          namespace of your own, such as `{sample}`"
      throwError "`{s}` is not a {what} lungo defines; lungo's are \
        {", ".intercalate (known.map (s!"`lungo.{·}`"))}. A {what} of your own belongs to a \
        namespace of your own"

/-- The record lungo reads for `decl`: `decl._lungo_<kind>`. Its last component starts with `_`,
so it is an internal detail, never exported. -/
meta def recordName (decl : Name) (kind : String) : Name :=
  .str decl s!"_lungo_{kind}"

end Lungo.Attr
