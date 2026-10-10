module

public import Lungo.Registry
public import Lungo.Spec.Contract
public import Lungo.Spec.Model
public import Lungo.Spec.State
public import Lungo.Trace
public import Lungo.Monitor
public import Lungo.Async

/-!
# The part of the library a program runs

Everything here imports only `Init` and declares no initializers, so a program that uses a
contract, a monitor or an async program carries exactly the code it calls. The attributes, which
need Lean's elaborator, are in `Lungo.Attr`; lungo never links them into a program.
-/
