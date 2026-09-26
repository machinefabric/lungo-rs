/-! IO: references, exceptions, monad transformers, files, and the environment. -/

def mayFail (n : Nat) : IO Nat := do
  if n > 3 then throw (IO.userError s!"too big: {n}")
  return n * 10

def withExcept (x : Int) : ExceptT String (StateM Nat) Int := do
  modify (· + 1)
  if x < 0 then throw s!"negative {x}"
  return x * 2

def main : IO Unit := do
  let r ← IO.mkRef (0 : Nat)
  for i in [1:11] do r.modify (· + i)
  IO.println s!"sum via ref: {← r.get}"
  let swapped ← r.swap 7
  IO.println s!"swap: {swapped} {← r.get}"
  for n in [1, 5, 2] do
    try
      let v ← mayFail n
      IO.println s!"ok {v}"
    catch e =>
      IO.println s!"caught: {e}"
  IO.println ((withExcept 5).run.run 0, (withExcept (-1)).run.run 10)
  let result ← (mayFail 9).toBaseIO
  match result with
  | .ok v => IO.println s!"unexpected {v}"
  | .error e => IO.println s!"toBaseIO error: {e}"
  let dir ← IO.FS.createTempDir
  let file := dir / "data.txt"
  IO.FS.writeFile file "line one\nline two\nλ three\n"
  let content ← IO.FS.readFile file
  IO.println (content.splitOn "\n")
  let h ← IO.FS.Handle.mk file .append
  h.putStrLn "appended"
  h.flush
  let lines ← IO.FS.lines file
  IO.println lines
  IO.println (← file.pathExists, ← (dir / "missing").pathExists)
  try
    let _ ← IO.FS.readFile (dir / "missing")
  catch e =>
    -- The message ends with the (random) temporary path; compare the error class only.
    IO.println s!"missing file error: {(toString e).splitOn "\n" |>.head!}"
  IO.FS.removeFile file
  IO.FS.removeDirAll dir
  IO.println (← dir.pathExists)
  IO.println ((← IO.getEnv "PATINA_CONFORMANCE_VAR"), (← IO.getEnv "PATINA_UNSET_VARIABLE_XYZ"))
  let stdout ← IO.getStdout
  stdout.putStrLn "direct to stdout handle"
  IO.eprintln "to stderr"
  let t0 ← IO.monoNanosNow
  let t1 ← IO.monoNanosNow
  IO.println (decide (t1 ≥ t0))
