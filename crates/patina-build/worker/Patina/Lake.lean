import Lean
import Lake.Load.Manifest
import Lake.Load.Workspace
import Lake.Config.InstallPath
import Patina.Cbor
import Patina.Diagnostics

/-!
Read-only access to the project's Lake workspace.

Lake owns package configuration, dependency resolution, and build artifacts. The worker reads
the locked manifest to learn which package owns each module and which files are the project's
inputs, and loads the root package's configuration through Lake to learn its default targets;
it never updates or rewrites anything.
-/
namespace Patina.LakeInfo

open Lean System Cbor

inductive PackageOrigin where
  /-- The workspace root package: the project the host is building. -/
  | root
  /-- A dependency on a local directory; its files are inputs of the build. -/
  | path (dir : String)
  /-- A locked remote dependency, identified by its manifest entry. -/
  | git (url : String) (rev : String)
  /-- The Lean toolchain itself (`Init`, `Std`, `Lean`, `Lake`). -/
  | toolchain
  deriving Inhabited

structure Package where
  name : String
  dir : FilePath
  origin : PackageOrigin
  configFile : Option FilePath
  deriving Inhabited

structure Workspace where
  root : FilePath
  packages : Array Package
  manifestFile : FilePath
  lakefile : FilePath
  sysrootSrc : FilePath

private def canonical (p : FilePath) : IO FilePath := do
  if ← p.pathExists then IO.FS.realPath p else return p

def load (root : FilePath) (sysroot : FilePath) : WorkerM Workspace := do
  let root ← liftIO .project "cannot resolve project root" (canonical root)
  let manifestFile := root / "lake-manifest.json"
  unless ← manifestFile.pathExists do
    fail .project s!"the Lake project has no lake-manifest.json; run `lake update` once and commit the manifest"
  let manifest ← liftIO .project "cannot read lake-manifest.json" (Lake.Manifest.load manifestFile)
  let lakefile ← if ← (root / "lakefile.lean").pathExists then pure (root / "lakefile.lean")
    else if ← (root / "lakefile.toml").pathExists then pure (root / "lakefile.toml")
    else fail .project "the Lake project has neither lakefile.lean nor lakefile.toml"
  let packagesDir := root / (manifest.packagesDir?.getD (FilePath.mk ".lake" / "packages"))
  let mut packages : Array Package := #[{
    name := manifest.name.toString (escape := false)
    dir := root
    origin := .root
    configFile := some lakefile
  }]
  for entry in manifest.packages do
    let (dir, origin) ← match entry.src with
      | .path dir => pure (root / dir, PackageOrigin.path dir.toString)
      | .git url rev _ subDir? =>
        let base := packagesDir / entry.dirName
        pure (match subDir? with | some sub => base / sub | none => base, PackageOrigin.git url rev)
    unless ← dir.pathExists do
      fail .project s!"Lake dependency '{entry.prettyName}' is not materialized at {dir}; \
        run `lake update` (or `cargo patina setup`) explicitly — builds never fetch dependencies"
    let dir ← liftIO .project "cannot resolve package directory" (canonical dir)
    packages := packages.push {
      name := entry.prettyName
      dir
      origin
      configFile := some (dir / entry.configFile)
    }
  let sysrootSrc ← liftIO .project "cannot resolve the toolchain source directory"
    (canonical (sysroot / "src" / "lean"))
  return { root, packages, manifestFile, lakefile, sysrootSrc }

/--
The root modules of the root package's default targets (`defaultTargets`), as Lake loads the
package's configuration (`lakefile.toml` or `lakefile.lean`): a library's `roots` and an
executable's `root`, in the order the defaults are declared, without duplicates. Dependencies
are not resolved or updated.
-/
def defaultRoots (root : FilePath) : WorkerM (Array Name) := do
  let (elan?, lean?, lake?) ← liftIO .project "cannot locate the Lean and Lake installations"
    (do return (← Lake.findInstall?))
  let some lean := lean? | fail .project "cannot locate the Lean installation of the Lake environment"
  let some lake := lake? | fail .project "cannot locate the Lake installation of the Lake environment"
  let lakeEnv ← match ← (Lake.Env.compute lake lean elan?).toBaseIO with
    | .ok env => pure env
    | .error e => fail .project s!"cannot compute the Lake environment: {e}"
  let (ws?, log) ← (Lake.loadWorkspaceRoot { lakeEnv, wsDir := root }).run? {}
  let some ws := ws?
    | fail .project s!"Lake cannot load the project's configuration:\n{"\n".intercalate (log.entries.map (·.toString)).toList}"
  let pkg := ws.root
  if pkg.defaultTargets.isEmpty then
    fail .request "the Lake package declares no default targets (`defaultTargets`); name the root modules explicitly"
  let mut roots : Array Name := #[]
  for target in pkg.defaultTargets do
    let some decl := pkg.findTargetDecl? target
      | fail .project s!"default target '{target}' is not a target of the package"
    let found : Array Name ←
      if let some lib := decl.leanLibConfig? then pure lib.roots
      else if let some exe := decl.leanExeConfig? then pure #[exe.root]
      else fail .request s!"default target '{target}' is neither a Lean library nor a Lean executable; \
        name the root modules explicitly"
    for r in found do
      unless roots.contains r do roots := roots.push r
  return roots

/-- Path components of `path` below `base`, if `path` lies inside `base`. -/
def relativeTo? (base path : FilePath) : Option (List String) :=
  let b := base.components.filter (· != "")
  let p := path.components.filter (· != "")
  if b.isPrefixOf p then some (p.drop b.length) else none

/-- Where a module's source lives, expressed without machine-specific absolute paths. -/
structure SourceLocation where
  package : String
  origin : PackageOrigin
  /-- `/`-separated path relative to the owning package directory. -/
  path : String
  absolute : FilePath
  deriving Inhabited

def Workspace.locate (ws : Workspace) (file : FilePath) : Option SourceLocation := Id.run do
  -- Prefer the most specific (longest) package directory: dependency checkouts live inside the
  -- root package's `.lake` directory.
  let mut best : Option (Nat × SourceLocation) := none
  for pkg in ws.packages do
    if let some rel := relativeTo? pkg.dir file then
      let depth := pkg.dir.components.length
      if best.all (·.1 < depth) then
        if rel.head? != some ".lake" || pkg.origin matches .git .. then
          best := some (depth, { package := pkg.name, origin := pkg.origin, path := "/".intercalate rel, absolute := file })
  if best.isNone then
    if let some rel := relativeTo? ws.sysrootSrc file then
      best := some (0, { package := "lean", origin := .toolchain, path := "/".intercalate rel, absolute := file })
  return best.map (·.2)

def PackageOrigin.toCbor : PackageOrigin → Value
  | .root => unitVariant "root"
  | .path dir => variant "path" [("dir", str dir)]
  | .git url rev => variant "git" [("url", str url), ("rev", str rev)]
  | .toolchain => unitVariant "toolchain"

def SourceLocation.toCbor (s : SourceLocation) : Value :=
  obj [("package", str s.package), ("origin", s.origin.toCbor), ("path", str s.path)]

/-- Whether edits to files at this location must trigger a rebuild of the generated code. -/
def PackageOrigin.isEditable : PackageOrigin → Bool
  | .root | .path _ => true
  | .git .. | .toolchain => false

/-- A project-relative, `/`-separated path for an input file of the build. -/
def Workspace.inputPath (ws : Workspace) (file : FilePath) : WorkerM String := do
  let file ← liftIO .project "cannot resolve input file" (canonical file)
  match relativeTo? ws.root file with
  | some rel => return "/".intercalate rel
  | none =>
    -- A path dependency outside the project directory: express it relative to the root.
    let rootComps := ws.root.components.filter (· != "")
    let fileComps := file.components.filter (· != "")
    let common := (rootComps.zip fileComps).takeWhile (fun (a, b) => a == b) |>.length
    if common == 0 then
      fail .project s!"input file {file} shares no directory with the project root"
    let ups := List.replicate (rootComps.length - common) ".."
    return "/".intercalate (ups ++ fileComps.drop common)

end Patina.LakeInfo
