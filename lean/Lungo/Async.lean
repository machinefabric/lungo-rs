module

/-!
# Async programs

A computation that needs the host to do something asynchronously — fetch a page, read a file,
wait for a user — yields a request and waits for the host's answer. `Program Op α` is such a
computation as data: either it is done with a value, or it asks the host to perform operation
`op` and continues with the answer, whose type `Interface.Ret op` depends on the operation.

An export returning `Program Op α` becomes a native async function in every language lungo
generates: the host passes a handler with one asynchronous method per constructor of `Op`, the
generated code awaits each method and resumes the program with its answer. Registering the
`Interface Op` instance with `@[lungo_facility "ns.name"]` makes `Op` a facility, and each of
its constructors an operation.

Because the program is data, Lean can reason about it without any host: `run` interprets it with
any handler (pure, stateful, a test double), and `Admits` relates it to the exchanges of
operations and answers it allows, so a claim can say what the program does for every possible
behaviour of the host, or only for hosts satisfying an assumption.
-/

@[expose] public section

namespace Lungo.Async

/-- The operations `Op` of an async facility, and what each one answers. -/
class Interface (Op : Type) where
  Ret : Op → Type

/-- A computation that is done, or waits for the host to perform an operation. -/
inductive Program (Op : Type) [Interface Op] (α : Type) : Type where
  | done (value : α)
  | call (op : Op) (resume : Interface.Ret op → Program Op α)

namespace Program

variable {Op : Type} [Interface Op] {α β : Type}

/-- Runs `p`, then the program `f` makes of its value. -/
def bind : Program Op α → (α → Program Op β) → Program Op β
  | .done a, f => f a
  | .call op k, f => .call op (fun r => bind (k r) f)

instance : Monad (Program Op) where
  pure := .done
  bind := bind

/-- Asks the host to perform `op` and returns its answer. -/
def perform (op : Op) : Program Op (Interface.Ret op) :=
  .call op .done

/-- Interprets `p` in monad `m`, performing each operation with `handler`. -/
def run {m : Type → Type} [Monad m] (handler : (op : Op) → m (Interface.Ret op)) :
    Program Op α → m α
  | .done a => pure a
  | .call op k => handler op >>= fun r => run handler (k r)

/-- One exchange with the host: an operation and the answer it was given. -/
def Exchange (Op : Type) [Interface Op] := (op : Op) × Interface.Ret op

/-- `p` ends with `a` when the host answers its operations as in `exchanges`, in order. -/
inductive Admits : Program Op α → List (Exchange Op) → α → Prop where
  | done (a : α) : Admits (.done a) [] a
  | call {op : Op} {k : Interface.Ret op → Program Op α} {r : Interface.Ret op}
      {rest : List (Exchange Op)} {a : α} :
      Admits (k r) rest a → Admits (.call op k) (⟨op, r⟩ :: rest) a

/-- The exchanges `p` has with a host whose answers `answer` gives, and the value it ends with. -/
def trace (answer : (op : Op) → Interface.Ret op) : Program Op α → List (Exchange Op) × α
  | .done a => ([], a)
  | .call op k =>
    let r := answer op
    let (rest, a) := trace answer (k r)
    (⟨op, r⟩ :: rest, a)

/-- Run with a host that always answers as `answer` does, `p` ends with the value `trace` gives,
after exactly the exchanges `trace` lists. -/
theorem admits_trace (answer : (op : Op) → Interface.Ret op) :
    ∀ p : Program Op α, Admits p (trace answer p).1 (trace answer p).2
  | .done a => .done a
  | .call op k => .call (admits_trace answer (k (answer op)))

/-- The value `run` computes with a pure handler is the one `trace` ends with. -/
theorem run_id (answer : (op : Op) → Interface.Ret op) :
    ∀ p : Program Op α, run (m := Id) (fun op => pure (answer op)) p = (trace answer p).2
  | .done _ => rfl
  | .call op k => run_id answer (k (answer op))

/-- A program admits at most one value for a given list of exchanges. -/
theorem admits_unique : ∀ {p : Program Op α} {t : List (Exchange Op)} {a b : α},
    Admits p t a → Admits p t b → a = b
  | _, _, _, _, .done _, .done _ => rfl
  | _, _, _, _, .call h₁, .call h₂ => admits_unique h₁ h₂

theorem bind_done (a : α) (f : α → Program Op β) : bind (.done a) f = f a := rfl

theorem bind_call (op : Op) (k : Interface.Ret op → Program Op α) (f : α → Program Op β) :
    bind (.call op k) f = .call op (fun r => bind (k r) f) := rfl

/-- The exchanges of `p >>= f` are those of `p`, then those of `f` applied to `p`'s value. -/
theorem admits_bind {f : α → Program Op β} :
    ∀ {p : Program Op α} {t₁ t₂ : List (Exchange Op)} {a : α} {b : β},
      Admits p t₁ a → Admits (f a) t₂ b → Admits (bind p f) (t₁ ++ t₂) b
  | _, _, _, _, _, .done _, h => h
  | _, _, _, _, _, .call h₁, h₂ => .call (admits_bind h₁ h₂)

end Program

end Lungo.Async
