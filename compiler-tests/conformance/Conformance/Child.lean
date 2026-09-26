/-! A helper process for `Conformance.Process`: its behavior is selected by its arguments. -/

partial def readAll (s : IO.FS.Stream) (acc : String) : IO String := do
  let line ← s.getLine
  if line.isEmpty then return acc else readAll s (acc ++ line)

def main (args : List String) : IO UInt32 := do
  match args with
  | ["echo", a, b] =>
    IO.println s!"{a}|{b}"
    IO.eprintln "child stderr"
    return 0
  | ["exit", n] => return n.toNat!.toUInt32
  | ["upper"] =>
    let input ← readAll (← IO.getStdin) ""
    IO.print input.toUpper
    return 0
  | ["env", v] =>
    IO.println (← IO.getEnv v)
    return 0
  | ["cwd"] =>
    IO.println (← IO.currentDir).fileName
    return 0
  | _ =>
    IO.println s!"child args: {args}"
    return 0
