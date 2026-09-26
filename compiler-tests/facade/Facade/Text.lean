namespace Facade

def textStats (s : String) : Nat × Nat × String × String × List String :=
  (s.length, s.utf8ByteSize, s.toUpper, String.ofList s.toList.reverse, s.splitOn " ")

def charInfo (c : Char) : UInt32 × Bool × Bool × Char := (c.val, c.isAlpha, c.isDigit, c.toUpper)

def joinWith (sep : String) (parts : Array String) : String := sep.intercalate parts.toList

def bytes (s : String) : ByteArray := s.toUTF8

def decode (b : ByteArray) : Option String := String.fromUTF8? b

def floats (xs : FloatArray) : Float := xs.foldl (· + ·) 0

def scaled (xs : FloatArray) (k : Float) : FloatArray := ⟨xs.data.map (· * k)⟩

end Facade
