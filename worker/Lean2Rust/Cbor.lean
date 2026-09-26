/-
Deterministic CBOR (RFC 8949) encoding and decoding for the lean2rust worker protocol.

The encoder produces the core deterministic encoding: preferred (shortest) argument
encodings, definite lengths only, and map keys ordered by the bytewise order of their
encodings. Only the data model used by the protocol is supported: unsigned and negative
integers, byte strings, text strings, arrays, text-keyed maps, booleans and null.
-/
namespace Lean2Rust.Cbor

inductive Value where
  | uint (n : Nat)
  /-- The negative integer `-1 - n`. -/
  | nint (n : Nat)
  | bytes (b : ByteArray)
  | text (s : String)
  | array (xs : Array Value)
  | map (entries : Array (String × Value))
  | bool (b : Bool)
  | null
  deriving Inhabited

def maxArgument : Nat := 2 ^ 64

private def pushBE (out : ByteArray) (n : Nat) (bytes : Nat) : ByteArray := Id.run do
  let mut out := out
  for i in [0:bytes] do
    let shift := 8 * (bytes - 1 - i)
    out := out.push ((n >>> shift) % 256).toUInt8
  return out

/-- Encodes a major type and argument using the shortest form. -/
def encodeHead (out : ByteArray) (major : Nat) (arg : Nat) : Except String ByteArray :=
  let initial := major <<< 5
  if arg < 24 then
    return out.push (initial + arg).toUInt8
  else if arg < 2 ^ 8 then
    return pushBE (out.push (initial + 24).toUInt8) arg 1
  else if arg < 2 ^ 16 then
    return pushBE (out.push (initial + 25).toUInt8) arg 2
  else if arg < 2 ^ 32 then
    return pushBE (out.push (initial + 26).toUInt8) arg 4
  else if arg < maxArgument then
    return pushBE (out.push (initial + 27).toUInt8) arg 8
  else
    throw s!"CBOR argument {arg} does not fit in 64 bits"

private def byteArrayLt (a b : ByteArray) : Bool := Id.run do
  let n := min a.size b.size
  for i in [0:n] do
    if a[i]! < b[i]! then return true
    if a[i]! > b[i]! then return false
  return a.size < b.size

private def byteArrayEq (a b : ByteArray) : Bool :=
  a.size == b.size && (List.range a.size).all fun i => a[i]! == b[i]!

mutual

partial def encodeInto (out : ByteArray) : Value → Except String ByteArray
  | .uint n => encodeHead out 0 n
  | .nint n => encodeHead out 1 n
  | .bytes b => return (← encodeHead out 2 b.size) ++ b
  | .text s =>
    let utf8 := s.toUTF8
    return (← encodeHead out 3 utf8.size) ++ utf8
  | .array xs => do
    let mut out ← encodeHead out 4 xs.size
    for x in xs do
      out ← encodeInto out x
    return out
  | .map entries => do
    let mut encoded : Array (ByteArray × ByteArray) := #[]
    for (k, v) in entries do
      encoded := encoded.push (← encodeInto .empty (.text k), ← encodeInto .empty v)
    let sorted := encoded.qsort fun a b => byteArrayLt a.1 b.1
    for i in [1:sorted.size] do
      if byteArrayEq sorted[i - 1]!.1 sorted[i]!.1 then
        throw "CBOR map contains a duplicate key"
    let mut out ← encodeHead out 5 sorted.size
    for (k, v) in sorted do
      out := out ++ k ++ v
    return out
  | .bool false => return out.push 0xf4
  | .bool true => return out.push 0xf5
  | .null => return out.push 0xf6

end

def encode (v : Value) : Except String ByteArray :=
  encodeInto .empty v

structure Decoder where
  data : ByteArray
  pos : Nat

abbrev DecodeM := StateT Decoder (Except String)

private def readByte : DecodeM UInt8 := do
  let s ← get
  if h : s.pos < s.data.size then
    set { s with pos := s.pos + 1 }
    return s.data[s.pos]
  else
    throw "truncated CBOR input"

private def readBE (bytes : Nat) : DecodeM Nat := do
  let mut n := 0
  for _ in [0:bytes] do
    n := n * 256 + (← readByte).toNat
  return n

private def readArgument (info : Nat) : DecodeM Nat := do
  if info < 24 then return info
  let n ← match info with
    | 24 => readBE 1
    | 25 => readBE 2
    | 26 => readBE 4
    | 27 => readBE 8
    | _ => throw s!"unsupported CBOR additional information {info}"
  let minimum := match info with
    | 24 => 24
    | 25 => 2 ^ 8
    | 26 => 2 ^ 16
    | _ => 2 ^ 32
  if n < minimum then throw "non-canonical CBOR argument encoding"
  return n

private def readSlice (len : Nat) : DecodeM ByteArray := do
  let s ← get
  if s.pos + len > s.data.size then throw "truncated CBOR input"
  set { s with pos := s.pos + len }
  return s.data.extract s.pos (s.pos + len)

partial def decodeValue (depth : Nat) : DecodeM Value := do
  if depth == 0 then throw "CBOR nesting exceeds the permitted depth"
  let initial ← readByte
  let major := initial.toNat >>> 5
  let info := initial.toNat % 32
  match major with
  | 0 => return .uint (← readArgument info)
  | 1 => return .nint (← readArgument info)
  | 2 => return .bytes (← readSlice (← readArgument info))
  | 3 =>
    let bytes ← readSlice (← readArgument info)
    match String.fromUTF8? bytes with
    | some s => return .text s
    | none => throw "CBOR text string is not valid UTF-8"
  | 4 =>
    let n ← readArgument info
    let mut xs := #[]
    for _ in [0:n] do
      xs := xs.push (← decodeValue (depth - 1))
    return .array xs
  | 5 =>
    let n ← readArgument info
    let mut entries := #[]
    for _ in [0:n] do
      let .text k ← decodeValue (depth - 1) | throw "CBOR map key is not a text string"
      if entries.any (·.1 == k) then throw s!"CBOR map contains duplicate key '{k}'"
      entries := entries.push (k, ← decodeValue (depth - 1))
    return .map entries
  | 7 =>
    match info with
    | 20 => return .bool false
    | 21 => return .bool true
    | 22 => return .null
    | _ => throw s!"unsupported CBOR simple value {info}"
  | _ => throw s!"unsupported CBOR major type {major}"

def decode (data : ByteArray) : Except String Value := do
  let (v, s) ← (decodeValue 256).run { data, pos := 0 }
  if s.pos != data.size then throw "trailing bytes after CBOR value"
  return v

/-! Construction helpers mirroring serde's default (externally tagged) representation. -/

def obj (fields : List (String × Value)) : Value := .map fields.toArray

/-- A struct-like enum variant: `{"name": {fields}}`. -/
def variant (name : String) (fields : List (String × Value)) : Value :=
  .map #[(name, obj fields)]

/-- A unit enum variant: `"name"`. -/
def unitVariant (name : String) : Value := .text name

def nat (n : Nat) : Value := .uint n

def str (s : String) : Value := .text s

def arr (xs : Array Value) : Value := .array xs

def opt : Option Value → Value
  | some v => v
  | none => .null

/-! Accessors used to decode requests. -/

def Value.field? (v : Value) (key : String) : Option Value :=
  match v with
  | .map entries => entries.find? (·.1 == key) |>.map (·.2)
  | _ => none

def Value.field (v : Value) (key : String) : Except String Value :=
  match v.field? key with
  | some x => return x
  | none => throw s!"missing field '{key}'"

def Value.asNat : Value → Except String Nat
  | .uint n => return n
  | _ => throw "expected an unsigned integer"

def Value.asString : Value → Except String String
  | .text s => return s
  | _ => throw "expected a text string"

def Value.asBool : Value → Except String Bool
  | .bool b => return b
  | _ => throw "expected a boolean"

def Value.asArray : Value → Except String (Array Value)
  | .array xs => return xs
  | _ => throw "expected an array"

def Value.checkFields (v : Value) (allowed : List String) : Except String Unit := do
  let .map entries := v | throw "expected a map"
  for (k, _) in entries do
    unless allowed.contains k do throw s!"unknown field '{k}'"

end Lean2Rust.Cbor
