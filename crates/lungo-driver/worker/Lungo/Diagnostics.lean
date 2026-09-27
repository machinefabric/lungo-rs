import Lungo.Cbor

/-!
Structured diagnostics reported by the worker.

Every failure the worker detects is reported as a diagnostic with an explicit category so that
the host can render Lean errors as Lean errors and backend or adapter failures as bridge
failures; the two are never conflated.
-/
namespace Lungo

open Cbor

inductive Severity where
  | error
  | warning
  | information
  deriving BEq, Inhabited

/-- What part of the pipeline produced the diagnostic. -/
inductive DiagnosticKind where
  /-- The Lean frontend, kernel, or compiler rejected the program. -/
  | lean
  /-- The project layout, manifest, or toolchain is invalid. -/
  | project
  /-- The request cannot be satisfied (unknown module, non-executable export, ...). -/
  | request
  /-- Lean produced compiler output this adapter does not accept. -/
  | adapter
  deriving BEq, Inhabited

structure Position where
  line : Nat
  column : Nat
  deriving Inhabited

structure Diagnostic where
  severity : Severity
  kind : DiagnosticKind
  message : String
  file : Option String := none
  position : Option Position := none
  declaration : Option String := none
  deriving Inhabited

def Severity.toCbor : Severity → Value
  | .error => unitVariant "error"
  | .warning => unitVariant "warning"
  | .information => unitVariant "information"

def DiagnosticKind.toCbor : DiagnosticKind → Value
  | .lean => unitVariant "lean"
  | .project => unitVariant "project"
  | .request => unitVariant "request"
  | .adapter => unitVariant "adapter"

def Diagnostic.toCbor (d : Diagnostic) : Value :=
  obj [
    ("severity", d.severity.toCbor),
    ("kind", d.kind.toCbor),
    ("message", str d.message),
    ("file", opt (d.file.map str)),
    ("position", opt (d.position.map fun p => obj [("line", nat p.line), ("column", nat p.column)])),
    ("declaration", opt (d.declaration.map str))
  ]

/-- A failure that aborts the worker with structured diagnostics. -/
structure Failure where
  diagnostics : Array Diagnostic

abbrev WorkerM := ExceptT Failure IO

def fail (kind : DiagnosticKind) (message : String) (declaration : Option String := none) : WorkerM α :=
  throw { diagnostics := #[{ severity := .error, kind, message, declaration }] }

def liftExcept (kind : DiagnosticKind) (context : String) (x : Except String α) : WorkerM α :=
  match x with
  | .ok a => return a
  | .error e => fail kind s!"{context}: {e}"

def liftIO (kind : DiagnosticKind) (context : String) (x : IO α) : WorkerM α := do
  match ← (x.toBaseIO : BaseIO (Except IO.Error α)) with
  | .ok a => return a
  | .error e => fail kind s!"{context}: {e}"

end Lungo
