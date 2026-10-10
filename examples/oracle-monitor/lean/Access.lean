import Lungo

/-!
Access control, specified once and used two ways.

`evaluate` decides a request against a policy, proved to decide `Permits` exactly. It is an
*oracle*: hand-written evaluators in other languages are tested against it, rather than trusted
on their own.

`observe` is a *runtime monitor* a host feeds events into, one at a time: it refuses an access by
a user not signed in, or one the policy does not permit, and a second sign-in. It is proved to
accept exactly the traces the session protocol allows, so what it reports is a real violation.
-/
namespace Access

open Lungo Lungo.Trace

inductive Effect where
  | allow
  | deny
  deriving Repr, DecidableEq

/-- A rule: what it says of a role performing an action. -/
structure Rule where
  role : String
  action : String
  effect : Effect
  deriving Repr, BEq

structure Request where
  role : String
  action : String
  deriving Repr, BEq

def Rule.covers (rule : Rule) (r : Request) : Bool := rule.role == r.role && rule.action == r.action

/-- A policy permits a request when a rule covering it allows it and no rule covering it denies
it. -/
@[lungo_spec "lungo.relation"]
def Permits (policy : List Rule) (r : Request) : Prop :=
  (∃ rule ∈ policy, rule.covers r = true ∧ rule.effect = .allow) ∧
    ∀ rule ∈ policy, rule.covers r = true → rule.effect ≠ .deny

/-- Whether `policy` permits `r`. -/
@[lungo_role "lungo.oracle"]
def evaluate (policy : List Rule) (r : Request) : Bool :=
  policy.any (fun rule => rule.covers r && rule.effect == .allow) &&
    policy.all (fun rule => !(rule.covers r && rule.effect == .deny))

@[lungo_claim "lungo.decides" subject evaluate spec Permits]
theorem evaluate_decides (policy : List Rule) (r : Request) : evaluate policy r = true ↔ Permits policy r := by
  simp only [evaluate, Permits, Bool.and_eq_true, List.any_eq_true, List.all_eq_true, Bool.not_eq_true',
    Bool.and_eq_false_iff, beq_iff_eq]
  constructor
  · rintro ⟨⟨rule, mem, covers, allow⟩, none⟩
    refine ⟨⟨rule, mem, covers, allow⟩, fun rule' mem' covers' deny => ?_⟩
    rcases none rule' mem' with h | h
    · simp [covers'] at h
    · exact (by simpa using h : rule'.effect ≠ .deny) deny
  · rintro ⟨⟨rule, mem, covers, allow⟩, none⟩
    refine ⟨⟨rule, mem, covers, allow⟩, fun rule' mem' => ?_⟩
    by_cases c : rule'.covers r = true
    · exact Or.inr (by simpa using none rule' mem' c)
    · exact Or.inl (by simpa using c)

inductive Event where
  | signIn (user role : String)
  | access (user action : String)
  | signOut (user : String)
  deriving Repr, BEq

/-- Who is signed in, with their role. -/
structure Session where
  users : List (String × String)
  deriving Repr, BEq

/-- The session after `e`; `none` when `e` breaks the protocol. -/
def next (policy : List Rule) (s : Session) (e : Event) : Option Session :=
  match e with
  | .signIn user role =>
    if s.users.any (·.1 == user) then none else some ⟨(user, role) :: s.users⟩
  | .access user action =>
    match s.users.find? (·.1 == user) with
    | some (_, role) => if evaluate policy ⟨role, action⟩ then some s else none
    | none => none
  | .signOut user =>
    if s.users.any (·.1 == user) then some ⟨s.users.filter (·.1 != user)⟩ else none

/-- No one signed in: the session a host starts monitoring from. -/
def start : Session := ⟨[]⟩

/-- The session protocol under `policy`: the event traces `next` allows from no one signed in. -/
@[lungo_spec "lungo.protocol"]
noncomputable def Sessions (policy : List Rule) : Protocol Event := (Reducer.mk start (next policy)).protocol

/-- Why `e` breaks the protocol in session `s`. -/
def reason (s : Session) (e : Event) : String :=
  match e with
  | .signIn user _ => s!"{user} is signed in already"
  | .access user action =>
    if s.users.any (·.1 == user) then s!"the policy does not permit {user} to {action}"
    else s!"{user} is not signed in"
  | .signOut user => s!"{user} is not signed in"

/-- The monitor of the session protocol under `policy`. -/
private def monitor (policy : List Rule) : Monitor Event Session := Monitor.ofReducer ⟨start, next policy⟩ reason

/-- The session after `e`, or the violation it is. -/
def observe (policy : List Rule) (s : Session) (e : Event) : Except String Session :=
  (monitor policy).step s e

/-- Starting from `start` and fed each event with `observe`, the monitor accepts exactly the traces
the session protocol allows: a violation it reports is one. -/
@[lungo_claim "lungo.monitors" subject start observe spec Sessions]
theorem observe_sound (policy : List Rule) : (Monitor.mk start (observe policy)).Sound (Sessions policy) :=
  Monitor.ofReducer_sound (Reducer.mk start (next policy)) reason

end Access
