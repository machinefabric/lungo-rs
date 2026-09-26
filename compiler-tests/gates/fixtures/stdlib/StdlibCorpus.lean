module

public import Std
meta import StdlibCorpus.Generate

/-!
A corpus containing every executable declaration of Lean's `Init` and `Std` libraries: compiling
this module puts the complete executable standard library into the program's closure.
(Lean reports that the corpus "uses `sorryAx`": `sorryAx` is itself one of `Init`'s executable
constants.)
-/

reference_all_executables 1000
