/-! Tasks, promises, thunks, and mutable state shared between threads. -/

def work (n : Nat) : Nat := (List.range (n * 1000)).foldl (· + ·) 0

def main : IO Unit := do
  let tasks := (List.range 8).map fun i => Task.spawn fun _ => work (i + 1)
  IO.println (tasks.map Task.get)
  let mapped := (Task.spawn fun _ => 21).map (· * 2)
  let bound := (Task.spawn fun _ => 5).bind fun x => Task.spawn fun _ => x + 100
  IO.println (mapped.get, bound.get)
  let ioTask ← IO.asTask (do return decide ((← IO.getNumHeartbeats) ≥ 0))
  match ioTask.get with
  | .ok v => IO.println s!"asTask ok {v}"
  | .error e => IO.println s!"asTask error {e}"
  let failing ← IO.asTask (throw (IO.userError "boom") : IO Nat)
  match ← IO.wait failing with
  | .ok v => IO.println s!"unexpected {v}"
  | .error e => IO.println s!"failed task: {e}"
  let counter ← IO.mkRef (0 : Nat)
  let incs ← (List.range 4).mapM fun _ => IO.asTask (do for _ in [0:250] do counter.modify (· + 1))
  for t in incs do discard <| IO.wait t
  IO.println s!"counter {← counter.get}"
  let p ← IO.Promise.new
  let waiter := p.result?.map fun v => v.map (· + 1)
  p.resolve (41 : Nat)
  IO.println waiter.get
  let th := Thunk.mk fun _ => work 3
  IO.println (th.get, th.get)
  let any ← IO.waitAny [Task.spawn (fun _ => 7), Task.spawn (fun _ => 7)]
  IO.println any
