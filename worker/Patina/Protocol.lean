import Patina.Cbor
import Patina.Diagnostics

/-!
The versioned parent/worker protocol.

A frame is `"PTNF"`, a one-byte frame kind (1 = request, 2 = response), the protocol version as
a little-endian `u32`, the payload length as a little-endian `u64`, and a deterministic CBOR
payload. The protocol version and the Bridge IR version are versioned independently.
-/
namespace Patina.Protocol

open Cbor

def version : Nat := 1

def requestKind : UInt8 := 1
def responseKind : UInt8 := 2

structure ExportPolicy where
  declarations : Array String
  modules : Array String

inductive Endian where
  | little
  | big
  deriving BEq

structure Target where
  triple : String
  pointerWidth : Nat
  endian : Endian

structure CompilerOption where
  name : String
  value : String

structure Request where
  bridgeVersion : String
  projectRoot : System.FilePath
  rootModules : Array String
  exports : ExportPolicy
  hostTriple : String
  target : Target
  compilerOptions : Array CompilerOption
  hermetic : Bool
  maxErrors : Nat
  runtimeExports : Array String

private def strings (v : Value) : Except String (Array String) := do
  (← v.asArray).mapM (·.asString)

def decodeRequest (v : Value) : Except String Request := do
  v.checkFields ["protocol_version", "bridge_version", "project_root", "root_modules",
    "export_policy", "host_triple", "target", "compiler_options", "hermetic", "diagnostics",
    "runtime_exports"]
  let protocolVersion ← (← v.field "protocol_version").asNat
  if protocolVersion != version then
    throw s!"request uses protocol version {protocolVersion}; this worker implements {version}"
  let policy ← v.field "export_policy"
  policy.checkFields ["declarations", "modules"]
  let target ← v.field "target"
  target.checkFields ["triple", "pointer_width", "endian"]
  let endian ← match ← (← target.field "endian").asString with
    | "little" => pure Endian.little
    | "big" => pure Endian.big
    | e => throw s!"unknown target endianness '{e}'"
  let pointerWidth ← (← target.field "pointer_width").asNat
  unless pointerWidth == 32 || pointerWidth == 64 do
    throw s!"unsupported target pointer width {pointerWidth}"
  let options ← (← (← v.field "compiler_options").asArray).mapM fun o => do
    o.checkFields ["name", "value"]
    return { name := ← (← o.field "name").asString, value := ← (← o.field "value").asString : CompilerOption }
  let diagnostics ← v.field "diagnostics"
  diagnostics.checkFields ["max_errors"]
  let projectRoot : System.FilePath := ← (← v.field "project_root").asString
  unless projectRoot.isAbsolute do throw "project_root must be an absolute path"
  return {
    bridgeVersion := ← (← v.field "bridge_version").asString
    projectRoot
    rootModules := ← strings (← v.field "root_modules")
    exports := {
      declarations := ← strings (← policy.field "declarations")
      modules := ← strings (← policy.field "modules")
    }
    hostTriple := ← (← v.field "host_triple").asString
    target := {
      triple := ← (← target.field "triple").asString
      pointerWidth
      endian
    }
    compilerOptions := options
    hermetic := ← (← v.field "hermetic").asBool
    maxErrors := ← (← diagnostics.field "max_errors").asNat
    runtimeExports := ← strings (← v.field "runtime_exports")
  }

private def readLE (data : ByteArray) (start count : Nat) : Nat := Id.run do
  let mut n := 0
  for i in [0:count] do
    n := n + (data[start + i]!.toNat <<< (8 * i))
  return n

private def pushLE (out : ByteArray) (n count : Nat) : ByteArray := Id.run do
  let mut out := out
  for i in [0:count] do
    out := out.push ((n >>> (8 * i)) % 256).toUInt8
  return out

/-- The four bytes every frame starts with. -/
def magic : ByteArray := "PTNF".toUTF8

def headerSize : Nat := 17

def decodeFrame (expectedKind : UInt8) (data : ByteArray) : Except String Value := do
  if data.size < headerSize then throw "truncated frame header"
  unless data.extract 0 magic.size == magic do
    throw "invalid frame magic"
  if data[4]! != expectedKind then throw s!"unexpected frame kind {data[4]!}"
  let frameVersion := readLE data 5 4
  if frameVersion != version then
    throw s!"frame uses protocol version {frameVersion}; this worker implements {version}"
  let length := readLE data 9 8
  if data.size - headerSize != length then
    throw s!"frame declares {length} payload bytes but contains {data.size - headerSize}"
  decode (data.extract headerSize data.size)

def encodeFrame (kind : UInt8) (payload : Value) : Except String ByteArray := do
  let body ← encode payload
  let header := magic.push kind
  let header := pushLE header version 4
  let header := pushLE header body.size 8
  return header ++ body

/-- Writes `bytes` to `path` atomically: a partial file is written and then renamed. -/
def writeAtomically (path : System.FilePath) (bytes : ByteArray) : IO Unit := do
  let partialPath := path.addExtension "partial"
  IO.FS.writeBinFile partialPath bytes
  IO.FS.rename partialPath path

end Patina.Protocol
