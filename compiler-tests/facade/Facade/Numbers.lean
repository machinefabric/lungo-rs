namespace Facade

def natOps (a b : Nat) : List Nat :=
  [a + b, a - b, a * b, a / b, a % b, Nat.gcd a b, a &&& b, a ||| b, a ^^^ b, a <<< (b % 64), a >>> (b % 64),
   Nat.log2 a, if a < b then 1 else 0]

def intOps (a b : Int) : List Int :=
  [a + b, a - b, a * b, a / b, a % b, a.tdiv b, a.tmod b, a.fdiv b, a.fmod b, -a, a.natAbs, a.toNat,
   if a < b then 1 else 0]

def natPow (a : Nat) (e : UInt8) : Nat := a ^ e.toNat

def fixedOps (a b : UInt64) (c d : Int32) (e : UInt8) (f : UInt16) (g : USize) : UInt64 × Int32 × UInt8 × UInt16 × USize :=
  (a * b + (a >>> (b % 64)) ^^^ (a / (b ||| 1)), c * d - c / (if d == 0 then 1 else d), e * 7 + e / 3, f - 12345 * f, g * 3 - 1)

def floatOps (x y : Float) : Float × Float × Float × Bool × UInt64 :=
  (x * y + x / y, Float.sqrt (Float.abs x), Float.floor x - Float.ceil y, x < y, x.toUInt64)

def float32Ops (x : Float32) : Float32 × Float := (x * x - 1.5, x.toFloat)

def signedMix (a : Int8) (b : Int16) (c : Int64) (d : ISize) : Int := a.toInt + b.toInt * 3 + c.toInt * 5 - d.toInt

end Facade
