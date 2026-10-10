import Lungo

/-!
Programs over a key-value store the host keeps, wherever it keeps it: the host answers each
operation asynchronously (a database, a remote service, a browser's storage). Since a program is
data, Lean runs it against `Model`, a store as a function from keys to values, and the claims say
what each program does to every store.
-/
namespace Store

open Lungo.Async

/-- What a program asks of the store. -/
inductive StoreOp where
  | get (key : String)
  | put (key value : String)

@[lungo_facility "store.kv"]
instance storeOps : Interface StoreOp where
  Ret
    | .get _ => Option String
    | .put _ _ => Unit

/-- A store: the value at each key, if any. -/
abbrev Contents := String → Option String

/-- `contents` with `value` at `key`. -/
private def Contents.set (contents : Contents) (key value : String) : Contents :=
  fun k => if k = key then some value else contents k

/-- The store as the host is expected to keep it: `get` reads, `put` writes. -/
@[lungo_spec "lungo.model"]
noncomputable def Model : (op : StoreOp) → StateM Contents (Interface.Ret op)
  | .get key => fun s => (s key, s)
  | .put key value => fun s => ((), s.set key value)

/-- Copies the value at `src` to `dst`; whether there was one. -/
def copy (src dst : String) : Program StoreOp Bool := do
  match ← Program.perform (StoreOp.get src) with
  | none => return false
  | some v =>
    let _ ← Program.perform (StoreOp.put dst v)
    return true

@[lungo_claim "lungo.law" subject copy spec Model]
theorem copy_model (s : Contents) (src dst : String) :
    (Program.run Model (copy src dst)).run s =
      match s src with
      | none => (false, s)
      | some v => (true, s.set dst v) := by
  cases h : s src <;> simp [copy, Program.perform, Program.run, Model, h, bind, Program.bind,
    StateT.run, StateT.bind, pure, StateT.pure]

/-- Exchanges the values at `a` and `b`; whether both had one (otherwise nothing changes). -/
def swap (a b : String) : Program StoreOp Bool := do
  match ← Program.perform (StoreOp.get a), ← Program.perform (StoreOp.get b) with
  | some x, some y =>
    let _ ← Program.perform (StoreOp.put a y)
    let _ ← Program.perform (StoreOp.put b x)
    return true
  | _, _ => return false

@[lungo_claim "lungo.law" subject swap spec Model]
theorem swap_model (s : Contents) (a b : String) (x y : String) (ha : s a = some x) (hb : s b = some y) :
    (Program.run Model (swap a b)).run s = (true, (s.set a y).set b x) := by
  simp [swap, Program.perform, Program.run, Model, ha, hb, bind, Program.bind, StateT.run, StateT.bind,
    pure, StateT.pure]

end Store
