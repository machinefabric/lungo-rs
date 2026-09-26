module

import all Lean
public meta import Lean.Language.Lean
public meta import Lean.Compiler.Options
public meta import Lean2Rust.BridgeIR
public meta import Lean2Rust.Capture

open Lean

public meta abbrev expectedLeanGitHash : String :=
  "5045d0056413266e57c625dcd7c365b10e377c52"

public meta unsafe def main (args : List String) : IO UInt32 := do
  let [moduleString, outputPath] := args | do
    IO.eprintln "usage: lean2rust-worker MODULE OUTPUT"
    return 2
  let outputFile : System.FilePath := outputPath
  let partialFile := outputFile.addExtension "partial"
  if ← outputFile.pathExists then IO.FS.removeFile outputFile
  if ← partialFile.pathExists then IO.FS.removeFile partialFile
  let some leanGitHash ← IO.getEnv "LEAN_GITHASH" | do
    IO.eprintln "lean2rust-worker must run inside the project's Lake environment"
    return 1
  if leanGitHash != expectedLeanGitHash then
    IO.eprintln s!"lean2rust-worker requires Lean commit {expectedLeanGitHash}; Lake selected {leanGitHash}"
    return 1
  initSearchPath (← findSysroot)
  enableInitializersExecution
  let moduleName := moduleString.toName
  let sourcePath ← findLean (← getSrcSearchPath) moduleName
  let input ← IO.FS.readFile sourcePath
  let inputCtx := Parser.mkInputContext input sourcePath.toString
  let opts := Compiler.compiler.postponeCompile.set ({} : Options) false
  let processingCtx : Language.ProcessingContext := { inputCtx with }
  let snap ← (Language.Lean.process (fun header => return .ok {
    mainModuleName := moduleName
    isModule := header.isModule
    imports := header.imports ++ #[{ module := `Lean2Rust.Capture, isMeta := true : Import }]
    opts
  }) none) processingCtx
  let tree := Language.toSnapshotTree snap
  let done ← tree.waitAll
  let _ ← pure done.get
  let messages : MessageLog := (tree.getAll.map (·.diagnostics.msgLog)).foldl (· ++ ·) {}
  for message in messages.unreported do
    IO.eprintln (← message.toString)
  if messages.hasErrors then return 1
  let some state := Language.Lean.waitForFinalCmdState? snap | do
    IO.eprintln "Lean frontend did not produce a final command state"
    return 1
  if state.messages.hasErrors then return 1
  let decls ← Lean2Rust.Capture.declarations.get
  let decls := decls.qsort (fun a b => a.name.toString < b.name.toString)
  let mut serialized := #[]
  for decl in decls do
    serialized := serialized.push (Lean2Rust.BridgeIR.declaration decl)
  let payloadJson := Json.mkObj [
    ("protocolVersion", toJson (1 : Nat)),
    ("birVersion", toJson Lean2Rust.BridgeIR.version),
    ("module", toJson moduleString),
    ("declarations", Json.arr serialized)
  ]
  let payload := payloadJson.compress.toUTF8
  if payload.size > UInt32.size then
    IO.eprintln "BIR response exceeds the protocol's maximum frame size"
    return 1
  let size := payload.size.toUInt32
  let mut frame := ByteArray.empty
  frame := frame.push 0x4c |>.push 0x32 |>.push 0x52 |>.push 0x42
  for shift in [0:4] do
    frame := frame.push ((size >>> (shift * 8).toUInt32).toUInt8)
  frame := frame ++ payload
  IO.FS.writeBinFile partialFile frame
  IO.FS.rename partialFile outputFile
  return 0
