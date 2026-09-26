module

public meta import Lean.Compiler.LCNF.Passes
public meta import Lean.CoreM

namespace Lean2Rust.Capture

open Lean Lean.Compiler.LCNF

public meta initialize declarations : IO.Ref (Array (Decl .impure)) ← IO.mkRef #[]

private meta def capturePass : Pass where
  phase := .impure
  name := `Lean2Rust.Capture.finalLCNF
  run := fun decls => do
    declarations.modify (· ++ decls)
    return decls

@[cpass] public meta def install : PassInstaller :=
  PassInstaller.installAtEnd .impure capturePass

end Lean2Rust.Capture
