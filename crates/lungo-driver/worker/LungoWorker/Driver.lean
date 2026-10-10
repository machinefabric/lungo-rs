import Lean
import LungoWorker.Cbor
import LungoWorker.Diagnostics
import LungoWorker.Protocol
import LungoWorker.Lake
import LungoWorker.BridgeIR
import LungoWorker.LCNFAdapter
import LungoWorker.Interface
import LungoWorker.Assurance

/-!
The worker driver: loads the project's compiled environment through Lean's own import
machinery and produces the versioned response for the host.

The worker runs inside `lake env` for the project, after Lake has elaborated, kernel-checked, and
compiled the requested modules. It performs no source parsing of its own.
-/
namespace LungoWorker.Driver

open Lean LungoWorker.Cbor System

/-- Version of this adapter for its Lean release; part of the worker identity. -/
def adapterVersion : Nat := 2

/-- The Lean release this adapter is validated against. -/
def supportedLeanVersion : String := "4.34.1"

def toolchainCbor : Value :=
  obj [
    ("lean_version", str Lean.versionString),
    ("lean_githash", str Lean.githash),
    ("adapter_version", nat adapterVersion),
    ("bir_version", nat BridgeIR.version)
  ]

structure ModuleNode where
  name : Name
  imports : Array Name
  location : Option LakeInfo.SourceLocation
  deriving Inhabited

structure Context where
  request : Protocol.Request
  /-- The program's root modules, as requested or resolved from Lake's default targets. -/
  roots : Array Name
  ws : LakeInfo.Workspace
  env : Environment
  modules : Array ModuleNode
  /-- The modules linked into the program, with the initialization phase each runs. -/
  linked : Std.HashMap Nat IRPhases

def runCore (env : Environment) (opts : Options) (x : CoreM α) : WorkerM (α × Environment) := do
  let ctx : Core.Context := {
    fileName := "<lungo>", fileMap := default, options := opts
    maxHeartbeats := 0, maxRecDepth := 8192
  }
  match ← (x.toIO ctx { env }).toBaseIO with
  | .ok (a, s) => return (a, s.env)
  | .error e => fail .adapter s!"Lean metaprogram failed while inspecting the environment: {e}"

/-- Runs `x` over `env`, reporting a failure as its message. -/
def coreIO (env : Environment) (opts : Options) {α : Type} (x : CoreM α) : IO (Except String α) := do
  let ctx : Core.Context := {
    fileName := "<lungo>", fileMap := default, options := opts
    maxHeartbeats := 0, maxRecDepth := 8192
  }
  match ← (x.toIO ctx { env }).toBaseIO with
  | .ok (a, _) => return .ok a
  | .error e => return .error (toString e)

def moduleGraph (env : Environment) (ws : LakeInfo.Workspace) : WorkerM (Array ModuleNode) := do
  let srcPath ← liftIO .project "cannot determine the Lean source search path" getSrcSearchPath
  -- The toolchain's own sources (`Init`, `Std`, `Lean`, `Lake`) ship with the installation.
  let srcPath := srcPath ++ [ws.sysrootSrc]
  let mut out := #[]
  for h : i in [0:env.header.moduleNames.size] do
    let name := env.header.moduleNames[i]
    let some data := env.header.moduleData[i]? | fail .adapter s!"module {name} has no data"
    let file? ← liftIO .project s!"cannot search for the source of {name}"
      (srcPath.findModuleWithExt "lean" name)
    let location ← match file? with
      | some file =>
        let file ← liftIO .project "cannot resolve source path" (IO.FS.realPath file)
        pure (ws.locate file)
      | none => pure none
    let imports := data.imports.foldl (init := #[]) fun acc i =>
      if acc.contains i.module then acc else acc.push i.module
    out := out.push { name, imports, location }
  return out

def ModuleNode.isLocal (m : ModuleNode) : Bool :=
  m.location.any fun l => l.origin matches .root

/--
The modules linked into the program and the initialization phase each one runs, following the
module initialization Lean's C backend emits. A `module` file runs its runtime phase, which
leaves out its `meta` imports (compile-time code); a legacy file runs every phase of all its
imports (`.all`). A module reached in both ways runs `.all`.

A `barrier` module is never linked, and nothing is reached through it: lungo's Lean library keeps
its attributes, and the parts of Lean they need, in modules that run only while a project is
compiled.
-/
def linkedPhases (env : Environment) (roots : Array Name) (barrier : Nat → Bool) :
    Std.HashMap Nat IRPhases := Id.run do
  let isModule (i : Nat) := (env.header.moduleData[i]?.map (·.isModule)).getD false
  let mut phases : Std.HashMap Nat IRPhases := {}
  let mut work : Array (Nat × IRPhases) := #[]
  for r in roots do
    if let some i := env.getModuleIdx? r then
      work := work.push (i.toNat, if isModule i.toNat then .runtime else .all)
  while h : work.size > 0 do
    let (i, p) := work[work.size - 1]
    work := work.pop
    match phases[i]?, p with
    | some .all, _ | some .runtime, .runtime => continue
    | _, _ => pure ()
    phases := phases.insert i p
    let some data := env.header.moduleData[i]? | continue
    for imp in data.imports do
      if p == .runtime && imp.isMeta then continue
      let some j := env.getModuleIdx? imp.module | continue
      let j := j.toNat
      if barrier j then continue
      work := work.push (j, if p == .all || !isModule j then .all else .runtime)
  return phases

def ModuleNode.toCbor (m : ModuleNode) : Value :=
  obj [
    ("name", BridgeIR.name m.name),
    ("imports", arr (m.imports.map BridgeIR.name)),
    ("source", opt (m.location.map (·.toCbor)))
  ]

def sourceRange (ctx : Context) (n : Name) : CoreM (Option Value) := do
  let some modIdx := ctx.env.getModuleIdxFor? n | return none
  let some node := ctx.modules[modIdx.toNat]? | return none
  let some loc := node.location | return none
  let some ranges ← findDeclarationRanges? n | return some (obj [("location", loc.toCbor), ("range", .null)])
  let r := ranges.range
  return some (obj [
    ("location", loc.toCbor),
    ("range", obj [
      ("start", obj [("line", nat r.pos.line), ("column", nat (r.pos.column + 1))]),
      ("end", obj [("line", nat r.endPos.line), ("column", nat (r.endPos.column + 1))])
    ])
  ])

/-- Resolves the export policy into the declarations that receive facades. -/
def selectExports (ctx : Context) (closureNames : NameSet) : CoreM (Array Name × Array Diagnostic) := do
  let env := ctx.env
  let mut out : NameSet := {}
  let mut errors : Array Diagnostic := #[]
  let requestError (decl : Option String) (message : String) : Diagnostic :=
    { severity := .error, kind := .request, declaration := decl, message }
  for d in ctx.request.exports.declarations do
    let n := d.toName
    if !env.contains n then
      errors := errors.push (requestError d s!"exported declaration '{d}' does not exist")
    else if !closureNames.contains n then
      errors := errors.push (requestError d s!"export '{d}' is non-executable: Lean produced no executable \
        implementation for it (it is a theorem, type, `noncomputable`, or otherwise erased declaration)")
    else
      out := out.insert n
  let exportedModules := ctx.request.exports.modules ++
    (if ctx.request.exports.roots then ctx.roots.map (·.toString (escape := false)) else #[])
  for m in exportedModules do
    let prefixName := m.toName
    -- Modules loaded only for their assurance records are not part of the program.
    let matching := (List.range ctx.modules.size).filter fun i =>
      let node := ctx.modules[i]!
      (prefixName == node.name || prefixName.isPrefixOf node.name) && ctx.linked.contains i
    if matching.isEmpty then
      errors := errors.push (requestError none s!"exported module '{m}' is not imported by the root modules")
    for i in matching do
      let some data := env.header.moduleData[i]? | continue
      for n in data.constNames do
        if closureNames.contains n && (← Interface.isAutoExportCandidate env n |>.run') then
          out := out.insert n
  let sorted := out.toArray.qsort fun a b => BridgeIR.nameString a < BridgeIR.nameString b
  return (sorted, errors)

structure TrustInfo where
  axioms : Array Name
  dependsOnSorry : Bool
  unsafeDeps : Array Name
  partialDeps : Array Name
  externs : Array String

def trust (ctx : Context) (idx : LCNFAdapter.IRIndex) (exports : Std.HashMap String Name) (n : Name) :
    WorkerM (TrustInfo × LCNFAdapter.IRIndex) := do
  let (axioms, _) ← runCore ctx.env {} (collectAxioms n)
  let cl ← LCNFAdapter.closure idx exports #[n]
  let mut unsafeDeps : NameSet := {}
  let mut partialDeps : NameSet := {}
  let mut externs : Array String := #[]
  let mut sorryDep := axioms.contains ``sorryAx
  for (d, modIdx) in cl.decls do
    if let .fdecl (info := { sorryDep? := some _ }) .. := d then sorryDep := true
    if let .extern _ _ _ data := d then
      match LCNFAdapter.cEntry data.entries with
      | some (.standard _ s) => externs := externs.push s
      | _ => externs := externs.push (BridgeIR.nameString d.name)
    let fromToolchain := ctx.modules[modIdx]?.any fun m => m.location.any (·.origin matches .toolchain)
    if let some o := LCNFAdapter.origin ctx.env d.name then
      if let some ci := ctx.env.find? o then
        if ci.isUnsafe && (o == n || !fromToolchain) then unsafeDeps := unsafeDeps.insert o
        if ci.isPartial && (o == n || !fromToolchain) then partialDeps := partialDeps.insert o
  let sortNames (s : NameSet) := s.toArray.qsort fun a b => BridgeIR.nameString a < BridgeIR.nameString b
  return ({
    axioms := axioms.filter (· != ``sorryAx) |>.qsort fun a b => BridgeIR.nameString a < BridgeIR.nameString b
    dependsOnSorry := sorryDep
    unsafeDeps := sortNames unsafeDeps
    partialDeps := sortNames partialDeps
    externs := (externs.qsort (· < ·)).toList.eraseDups.toArray
  }, cl.index)

def run (request : Protocol.Request) : WorkerM Value := do
  let some githash ← (IO.getEnv "LEAN_GITHASH" : IO _)
    | fail .project "lungo-worker must run inside the project's Lake environment (`lake env`)"
  if githash != Lean.githash then
    fail .project s!"toolchain mismatch: this worker was built with Lean {Lean.versionString} ({Lean.githash}) \
      but the project's Lake environment provides Lean commit {githash}"
  let sysroot ← liftIO .project "cannot locate the Lean installation" (findSysroot)
  liftIO .project "cannot initialize the Lean search path" (initSearchPath sysroot)
  let ws ← LakeInfo.load request.projectRoot sysroot
  let roots ← match request.roots with
    | .modules names => pure (names.map String.toName)
    | .defaultTargets => LakeInfo.defaultRoots ws.root
  for r in roots do
    if r.isAnonymous then fail .request "a root module name is empty"
  liftIO .adapter "cannot enable initializers" (unsafe enableInitializersExecution)
  let opts := request.compilerOptions.foldl (init := ({} : Options)) fun o opt =>
    o.set opt.name.toName opt.value
  -- Modules loaded only for the assurance records they hold (laws stated apart from the
  -- program): imported into the environment, never linked into the program.
  let assuranceModules := request.assuranceModules.map String.toName
  for m in assuranceModules do
    if m.isAnonymous then fail .request "an assurance module name is empty"
    if roots.contains m then
      fail .request s!"{m} is both a root module and an assurance module; a root module's records are read anyway"
  let env ← match ← (importModules ((roots ++ assuranceModules).map ({ module := · })) opts
      (level := .private) (loadExts := true)).toBaseIO with
    | .ok env => pure env
    | .error e => fail .lean s!"cannot load the compiled Lean environment of the root modules: {e}"
  let modules ← moduleGraph env ws
  for r in roots do
    let some idx := env.getModuleIdx? r | fail .request s!"root module {r} was not loaded"
    unless modules[idx.toNat]!.isLocal do
      fail .request s!"root module {r} is not a module of the project's root package"
  for m in assuranceModules do
    let some idx := env.getModuleIdx? m | fail .request s!"assurance module {m} was not loaded"
    if modules[idx.toNat]!.location.all (·.origin matches .toolchain) then
      fail .request s!"assurance module {m} is not a module of a package of the Lake workspace"
  let packageOf (i : Nat) : Option String := modules[i]?.bind (·.location.map (·.package))
  let library ← liftExcept .project "cannot read lungo's Lean library" (Assurance.findLibrary env packageOf)
  let barriers := library.map (·.barriers) |>.getD #[]
  let linked := linkedPhases env roots (barriers.contains ·)
  if library.isSome && env.contains `Lungo.Async.Program.done then
    let (_, _) ← runCore env opts Interface.checkProgramLayout.run'
  let records := library.map (Assurance.readRecords env) |>.getD {}
  let ctx : Context := { request, roots, ws, env, modules, linked }
  -- Compilation roots: every runtime declaration of the project's linked local modules, every
  -- initializer of every linked module, and explicitly exported declarations.
  let mut idx : LCNFAdapter.IRIndex := { env }
  let mut compileRoots : Array Name := #[]
  for h : i in [0:modules.size] do
    let some phases := linked[i]? | continue
    if modules[i].isLocal then
      let (decls, idx') := LCNFAdapter.moduleDecls idx i
      idx := idx'
      let runtime := decls.toArray.filter fun (n, _) =>
        phases == .all || !(isMarkedMeta env n || (LCNFAdapter.origin env n).any (isMarkedMeta env))
      compileRoots := compileRoots ++ (runtime.map (·.1)).qsort (fun a b => BridgeIR.nameString a < BridgeIR.nameString b)
  let mut initCbor := #[]
  for h : i in [0:modules.size] do
    let some phases := linked[i]? | continue
    let inits := LCNFAdapter.initializers env i phases
    for init in inits do compileRoots := compileRoots ++ init.decls
    initCbor := initCbor.push (obj [
      ("name", BridgeIR.name modules[i].name),
      ("imports", arr (modules[i].imports.map BridgeIR.name)),
      ("initializers", arr (inits.map fun
        | .io d => .map #[("io", BridgeIR.name d)]
        | .value d f => variant "value" [("decl", BridgeIR.name d), ("init_fn", BridgeIR.name f)]))
    ])
  for d in request.exports.declarations do
    let n := d.toName
    if env.contains n && (idx.env.getModuleIdxFor? n).isSome then
      if let some modIdx := env.getModuleIdxFor? n then
        let (m, idx') := LCNFAdapter.moduleDecls idx modIdx.toNat
        idx := idx'
        if m.contains n then compileRoots := compileRoots.push n
  -- Every operation of a facility the program's modules declare is compiled, whether or not
  -- the program calls it, so a facility's interface does not change with what reaches it.
  for (decl, _) in records.operations.toArray.qsort (fun a b => BridgeIR.nameString a.1 < BridgeIR.nameString b.1) do
    if let some modIdx := env.getModuleIdxFor? decl then
      if linked.contains modIdx.toNat then compileRoots := compileRoots.push decl
  let exportMap := LCNFAdapter.exportedSymbols env (linked.contains ·)
  -- Compiled Lean functions the runtime itself calls: `IO.Error.toString` renders uncaught
  -- `IO` errors (as Lean's runtime does), and the oracle backend renders numbers and builds
  -- user errors with the others.
  let helperNames := #[``IO.Error.toString, ``Nat.reprFast, ``Int.repr, ``IO.userError]
  compileRoots := compileRoots ++ helperNames
  -- Lean definitions the target runtime calls through their `@[export]` symbols.
  let mut runtimeExports : Array (String × Name) := #[]
  for sym in request.runtimeExports do
    let some decl := exportMap[sym]?
      | fail .adapter s!"the lungo runtime calls the Lean definition exported as `{sym}`, which this toolchain does not define"
    runtimeExports := runtimeExports.push (sym, decl)
    compileRoots := compileRoots.push decl
  let cl ← LCNFAdapter.closure idx exportMap compileRoots
  idx := cl.index
  let closureNames : NameSet := cl.decls.foldl (fun s (d, _) => s.insert d.name) {}
  -- Lean's compiler reuses specializations compiled in any imported module, so the program may
  -- run code of a module it does not link (one loaded only for its assurance records, or reached
  -- only through lungo's compile-time attributes). Such code is plain compiled code and runs as
  -- it is; what cannot run is a value the module's initializer computes, since that initializer
  -- is not part of the program.
  let mut metadataOnly : Array Assurance.Violation := #[]
  let mut reported : NameSet := {}
  -- The modules owning such code are part of the program, with nothing to initialize.
  let mut unlinkedOwners : Std.HashSet Nat := {}
  for (_, modIdx) in cl.decls do
    unless linked.contains modIdx do unlinkedOwners := unlinkedOwners.insert modIdx
  for i in unlinkedOwners.toArray.qsort (· < ·) do
    initCbor := initCbor.push (obj [
      ("name", BridgeIR.name modules[i]!.name),
      ("imports", arr (modules[i]!.imports.map BridgeIR.name)),
      ("initializers", arr #[])
    ])
  for (d, modIdx) in cl.decls do
    if linked.contains modIdx then continue
    let n := d.name
    let initialized := isIOUnitBuiltinInitFn env n || isIOUnitInitFn env n ||
      (getBuiltinInitFnNameFor? env n <|> getInitFnNameFor? env n).isSome
    if initialized && !reported.contains n then
      reported := reported.insert n
      let message := s!"the program uses `{n}`, which module {modules[modIdx]!.name} computes when it is \
        initialized; lungo does not link that module (it is loaded only for its assurance records, or is \
        compile-time only), so nothing would initialize it: import the module from the program's modules"
      metadataOnly := metadataOnly.push { kind := .metadataOnlyDependency, declaration := some n, message }
  let declsCbor ← cl.decls.mapM fun (d, modIdx) =>
    liftExcept .adapter s!"cannot represent compiler declaration '{d.name}'"
      (LCNFAdapter.encodeDecl env exportMap d modIdx)
  -- Exports and their facades.
  let ((exports, exportErrors), _) ← runCore env opts (selectExports ctx closureNames)
  unless exportErrors.isEmpty do throw { diagnostics := exportErrors }
  let mut exportsCbor := #[]
  let mut facadeState : Interface.State := {}
  for n in exports do
    let some modIdx := env.getModuleIdxFor? n | fail .adapter s!"export '{n}' has no owning module"
    let (m, idx') := LCNFAdapter.moduleDecls idx modIdx.toNat
    idx := idx'
    let some d := m[n]? | fail .adapter s!"export '{n}' has no compiler declaration"
    let ((sig, st), _) ← runCore env opts ((Interface.signature n d.params).run facadeState).run'
    facadeState := st
    let (t, idx') ← trust ctx idx exportMap n
    idx := idx'
    let (source, _) ← runCore env opts (sourceRange ctx n)
    exportsCbor := exportsCbor.push (obj [
      ("name", BridgeIR.name n),
      ("module", BridgeIR.name (modules[modIdx.toNat]!.name)),
      ("lean_type", str sig.leanType),
      ("type_params", arr (sig.typeParams.map str)),
      ("params", arr (sig.params.map (·.toCbor))),
      ("result", sig.result.toCbor),
      ("source", opt source),
      ("trust", obj [
        ("axioms", arr (t.axioms.map BridgeIR.name)),
        ("depends_on_sorry", .bool t.dependsOnSorry),
        ("unsafe_dependencies", arr (t.unsafeDeps.map BridgeIR.name)),
        ("partial_dependencies", arr (t.partialDeps.map BridgeIR.name)),
        ("extern_dependencies", arr (t.externs.map str))
      ])
    ])
  -- Extern requirements and source metadata for every compiled declaration's origin.
  let mut externsCbor := #[]
  let mut origins : NameSet := {}
  for (d, _) in cl.decls do
    if let some o := LCNFAdapter.origin env d.name then origins := origins.insert o
    if let .extern f xs ty data := d then
      let some entry := LCNFAdapter.cEntry data.entries | fail .adapter s!"extern '{f}' has no C entry"
      let o := LCNFAdapter.origin env f
      let (leanType, _) ← runCore env opts do
        match o.bind env.find? with
        | some ci => return some (toString (← (Meta.ppExpr ci.type).run'))
        | none => return none
      let (source, _) ← runCore env opts (match o with | some o => sourceRange ctx o | none => pure none)
      -- The Rust-facing signature an application-provided implementation receives. Externs whose
      -- declaration has no source-level constant (or whose type does not expose its compiled
      -- arity) have none; they can only be provided by the runtime or by `@[export]`ed Lean code.
      let facade ← match o with
        | some o =>
          try
            let ((sig, st), _) ← runCore env opts ((Interface.signature o xs).run facadeState).run'
            facadeState := st
            pure (some (obj [
              ("type_params", arr (sig.typeParams.map str)),
              ("params", arr (sig.params.map (·.toCbor))),
              ("result", sig.result.toCbor)
            ]))
          catch _ => pure none
        | none => pure none
      let operation := (o.bind records.operations.find?).map fun op =>
        obj [("facility", BridgeIR.name op.facility)]
      externsCbor := externsCbor.push (obj [
        ("operation", opt operation),
        ("declaration", BridgeIR.name f),
        ("entry", BridgeIR.externEntry entry),
        ("lean_type", opt (leanType.map str)),
        ("params", arr (← xs.mapM fun p => liftExcept .adapter "extern parameter" (BridgeIR.param p))),
        ("result", ← liftExcept .adapter "extern result" (BridgeIR.irType ty)),
        ("source", opt source),
        ("facade", opt facade)
      ])
  let ((types, _), _) ← runCore env opts ((Interface.describePending).run facadeState).run'
  let mut sourcesCbor := #[]
  for o in origins.toArray.qsort (fun a b => BridgeIR.nameString a < BridgeIR.nameString b) do
    let (source, _) ← runCore env opts (sourceRange ctx o)
    if let some s := source then
      sourcesCbor := sourcesCbor.push (obj [("name", BridgeIR.name o), ("source", s)])
  -- Inputs of the build: the project configuration and every editable module source.
  let mut inputs : Array String := #[]
  inputs := inputs.push (← ws.inputPath (ws.root / "lean-toolchain"))
  inputs := inputs.push (← ws.inputPath ws.lakefile)
  inputs := inputs.push (← ws.inputPath ws.manifestFile)
  for pkg in ws.packages do
    if pkg.origin matches .path _ then
      if let some cfg := pkg.configFile then
        if ← cfg.pathExists then inputs := inputs.push (← ws.inputPath cfg)
  for m in modules do
    if let some loc := m.location then
      if loc.origin.isEditable then inputs := inputs.push (← ws.inputPath loc.absolute)
  let inputFiles := (inputs.qsort (· < ·)).toList.eraseDups.toArray
  -- The program entry point, when a root module defines Lean's `main`.
  let entryPoint ← match env.find? `main, env.getModuleIdxFor? `main with
    | some ci, some modIdx =>
      if roots.contains modules[modIdx.toNat]!.name then
        let (takesArgs, returnsCode) ← match ci.type with
          | .forallE _ (.app (.const ``List _) (.const ``String _)) (.app (.const ``IO _) (.const r _)) _ =>
            pure (true, r == ``UInt32)
          | .app (.const ``IO _) (.const r _) => pure (false, r == ``UInt32)
          | _ => fail .request "`main` must have type `(List String →)? IO (UInt32 | Unit | PUnit)`"
        pure (some (obj [("declaration", str "main"), ("takes_args", .bool takesArgs),
          ("returns_exit_code", .bool returnsCode)]))
      else pure none
    | _, _ => pure none
  -- Native symbols of Lean's own C backend, used when the facade runs on Lean's native
  -- compiler output (the `LeanOracle` mode).
  let cName (n : Name) : String :=
    match getExportNameFor? env n with
    | some (.str .anonymous s) => s
    | _ => if n == `main then "_lean_main" else getSymbolStem env n
  for h in helperNames do
    unless closureNames.contains h do
      fail .adapter s!"'{h}' has no compiled Lean implementation in this toolchain"
  let symbolNames := exports ++ helperNames ++ (if entryPoint.isSome then #[`main] else #[])
  let oracleCbor := obj [
    ("symbols", arr (symbolNames.map fun n => obj [("name", BridgeIR.name n), ("symbol", str (cName n))])),
    ("module_initializers", arr (← roots.mapM fun r => do
      let some idx := env.getModuleIdx? r | fail .request s!"root module {r} was not loaded"
      let some data := env.header.moduleData[idx.toNat]? | fail .adapter s!"root module {r} has no data"
      let phases := if data.isModule then IRPhases.runtime else IRPhases.all
      pure (obj [("module", BridgeIR.name r),
        ("symbol", str (mkModuleInitializationFunctionName r (env.getModulePackageByIdx? idx) phases))])))
  ]
  -- Assurance records, checked against the environment.
  let origin (decl : Name) : CoreM Value := do
    let some modIdx := env.getModuleIdxFor? decl | throwError "`{decl}` belongs to no module"
    let node := modules[modIdx.toNat]!
    return obj [("module", BridgeIR.name node.name),
      ("package", opt (node.location.map fun l => str l.package)),
      ("source", opt (← sourceRange ctx decl))]
  let exportSet : NameSet := exports.foldl (·.insert ·) {}
  let assuranceCbor ← match library with
    | none => pure (obj [("library", .null), ("specs", arr #[]), ("claims", arr #[]),
        ("facilities", arr #[]), ("operations", arr #[]), ("assumptions", arr #[]),
        ("roles", arr #[]), ("definitions", arr #[]), ("violations", arr (metadataOnly.map (·.toCbor)))])
    | some lib =>
      let input : Assurance.Input := {
        env, opts, lib, records
        exports := exportSet
        closureNames, origin
        runCore := coreIO env opts
        inWorkspace := fun n => match env.getModuleIdxFor? n with
          | some i => modules[i.toNat]!.location.any fun l => !(l.origin matches .toolchain)
          | none => false
      }
      let out ← liftIO .adapter "cannot check the assurance records" (Assurance.analyze input)
      let out ← liftExcept .adapter "cannot check the assurance records" out
      let .map fields := out.value | fail .adapter "assurance records are not a map"
      pure (.map (fields.push ("violations", arr ((out.violations ++ metadataOnly).map (·.toCbor)))))
  return obj [
    ("root_modules", arr (roots.map BridgeIR.name)),
    ("assurance", assuranceCbor),
    ("oracle", oracleCbor),
    ("module_graph", arr (modules.map (·.toCbor))),
    ("input_files", arr (inputFiles.map str)),
    ("bir", obj [
      ("bir_version", nat BridgeIR.version),
      ("modules", arr initCbor),
      ("declarations", arr declsCbor)
    ]),
    ("extern_requirements", arr externsCbor),
    ("source_metadata", arr sourcesCbor),
    ("interface", obj [("exports", arr exportsCbor), ("types", arr types)]),
    ("entry_point", opt entryPoint),
    ("runtime_exports", arr (runtimeExports.map fun (sym, d) =>
      obj [("symbol", str sym), ("declaration", BridgeIR.name d)]))
  ]

def response (outcome : Value) (diagnostics : Array Diagnostic) : Value :=
  obj [
    ("protocol_version", nat Protocol.version),
    ("toolchain", toolchainCbor),
    ("diagnostics", arr (diagnostics.map (·.toCbor))),
    ("outcome", outcome)
  ]

/-- Entry point: reads the request frame, runs the worker, and writes the response frame. -/
def main (requestPath responsePath : FilePath) : IO UInt32 := do
  if ← responsePath.pathExists then IO.FS.removeFile responsePath
  let bytes ← IO.FS.readBinFile requestPath
  let request ← match Protocol.decodeFrame Protocol.requestKind bytes >>= Protocol.decodeRequest with
    | .ok r => pure r
    | .error e =>
      IO.eprintln s!"lungo-worker: invalid request: {e}"
      return 2
  let outcome ← (run request).run
  let payload := match outcome with
    | .ok success => response (.map #[("success", success)]) #[]
    | .error failure =>
      let diags := if request.maxErrors > 0 then failure.diagnostics.extract 0 request.maxErrors
        else failure.diagnostics
      response (unitVariant "failure") diags
  match Protocol.encodeFrame Protocol.responseKind payload with
  | .ok frame =>
    Protocol.writeAtomically responsePath frame
    return 0
  | .error e =>
    IO.eprintln s!"lungo-worker: cannot encode the response: {e}"
    return 3

end LungoWorker.Driver
