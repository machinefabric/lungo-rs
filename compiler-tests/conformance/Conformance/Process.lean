/-! Child processes: spawning, pipes, standard input, exit codes, environment, working
directory, and spawn failures. The child is the `child` helper named by
`PATINA_CONFORMANCE_CHILD`. -/

def main : IO UInt32 := do
  let some child ← IO.getEnv "PATINA_CONFORMANCE_CHILD"
    | IO.eprintln "PATINA_CONFORMANCE_CHILD is not set"; return 2
  -- Captured output and error streams.
  let out ← IO.Process.output { cmd := child, args := #["echo", "a b", "ünï"] }
  IO.println (out.exitCode, out.stdout, out.stderr)
  -- Exit codes.
  for code in [0, 1, 7, 255] do
    let out ← IO.Process.output { cmd := child, args := #["exit", toString code] }
    IO.println s!"exit {code} → {out.exitCode}"
  -- Piped standard input, closed when the handle is released.
  let p ← IO.Process.spawn { cmd := child, args := #["upper"], stdin := .piped, stdout := .piped }
  let (stdin, p) ← p.takeStdin
  stdin.putStr "first line\nsecond ✓ line\n"
  stdin.flush
  let upper ← p.stdout.readToEnd
  IO.println (upper, ← p.wait)
  -- Environment: set, inherited, and removed variables.
  let env (v : String) (e : Array (String × Option String)) := do
    let out ← IO.Process.output { cmd := child, args := #["env", v], env := e }
    return out.stdout.trimAscii.toString
  IO.println (← env "PTN_CHILD_VAR" #[("PTN_CHILD_VAR", some "set!")])
  IO.println (← env "PATINA_CONFORMANCE_VAR" #[])
  IO.println (← env "PATINA_CONFORMANCE_VAR" #[("PATINA_CONFORMANCE_VAR", none)])
  -- Working directory.
  let out ← IO.Process.output { cmd := child, args := #["cwd"], cwd := some "Conformance" }
  IO.println out.stdout.trimAscii.toString
  -- `IO.Process.run` fails on a nonzero exit code.
  try
    let _ ← IO.Process.run { cmd := child, args := #["exit", "3"] }
    IO.println "run succeeded"
  catch e => IO.println s!"run failed: {decide ((toString e).length > 0)}"
  -- A program that does not exist: Lean's runtime reports it from the child process.
  try
    let out ← IO.Process.output { cmd := "patina-no-such-program-xyz" }
    IO.println (out.exitCode, out.stdout, out.stderr)
  catch e => IO.println s!"spawn failed: {e}"
  return 0
