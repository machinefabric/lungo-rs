module

public import Lungo.Attr.Attributes

/-!
# The attributes, at compile time only

These modules need Lean's elaborator and run only while a project is compiled. lungo never links
them, or the parts of Lean they import, into a program: their initializers never run in it, and a
program that uses a value one of them initializes is refused (`LNG0710`).
-/
