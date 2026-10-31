import Lake.Load.Manifest

/-!
The parts of Lean's and Lake's interface that differ between the Lean releases lungo supports, as
Lean 4.34.1 has them. lungo builds the worker from its shared sources and the `Compat` module of
the project's release; every release's module has the same declarations.
-/
namespace LungoWorker.Compat

open System

/-- Where a locked package comes from, in a shape every supported release can express. -/
inductive Source where
  /-- A directory, relative to the project; `copy` when Lake copies it into the packages
  directory (`copy = true`) and loads it from there. -/
  | path (dir : FilePath) (copy : Bool)
  /-- A Git repository at a locked revision, optionally a subdirectory of it. -/
  | git (url : String) (rev : String) (subDir? : Option FilePath)

/-- Lake 4.34.1 never copies a path dependency. -/
def source : Lake.PackageEntrySrc → Source
  | .path dir => .path dir false
  | .git url rev _ subDir? => .git url rev subDir?

end LungoWorker.Compat
