/-! Natural numbers, integers, and fixed-width arithmetic at the boundaries of every representation. -/

def natCases : List (Nat × Nat) :=
  [(0, 0), (1, 0), (7, 3), (2^63 - 1, 1), (2^63, 2^63), (2^64 + 5, 2^32), (10^30, 10^15 + 7),
   (123456789012345678901234567890, 987654321)]

def intCases : List (Int × Int) :=
  [(0, 1), (-7, 2), (7, -2), (-7, -2), (2^31, -1), (-(2^31), -1), (-(2^63), 3), (10^20, -(10^19)),
   (-(10^25), 7), (5, 0), (-5, 0)]

def showNat (a b : Nat) : String :=
  s!"{a} {b}: + {a + b} - {a - b} * {a * b} / {a / b} % {a % b} gcd {Nat.gcd a b} " ++
  s!"land {a &&& b} lor {a ||| b} xor {a ^^^ b} shl {a <<< (b % 70)} shr {a >>> (b % 70)} " ++
  s!"log2 {Nat.log2 a} lt {decide (a < b)} le {decide (a ≤ b)} eq {decide (a = b)}"

def showInt (a b : Int) : String :=
  s!"{a} {b}: + {a + b} - {a - b} * {a * b} / {a / b} % {a % b} tdiv {a.tdiv b} tmod {a.tmod b} " ++
  s!"fdiv {a.fdiv b} fmod {a.fmod b} neg {-a} natAbs {a.natAbs} toNat {a.toNat} lt {decide (a < b)}"

def fixedWidth : List String :=
  let u8 : UInt8 := 250
  let u16 : UInt16 := 65530
  let u32 : UInt32 := 4294967290
  let u64 : UInt64 := 18446744073709551610
  let i8 : Int8 := -128
  let i64 : Int64 := -9223372036854775808
  [ s!"{u8 + 10} {u8 * 3} {u8 / 0} {u8 % 0} {u8 <<< 9} {u8 >>> 3} {~~~u8} {u8.toNat}",
    s!"{u16 + 10} {u16 * 3} {u16 - 65535} {u16.log2} {u16.toUInt8}",
    s!"{u32 + 10} {u32 * 3} {u32 <<< 33} {u32.toUInt64 * 4}",
    s!"{u64 + 10} {u64 * 3} {u64 / 7} {u64.toNat + 1} {(0 : UInt64) - 1}",
    s!"{i8 - 1} {i8 / -1} {i8 % -1} {-i8} {i8.toInt} {(100 : Int8) + 100}",
    s!"{i64 / -1} {i64 - 1} {i64.toInt - 1} {(-7 : Int64) / 2} {(-7 : Int64) % 2} {(-7 : Int64) >>> 1}",
    s!"{(300 : Nat).toUInt8} {(2^70 + 3 : Nat).toUInt64} {(-1 : Int).toInt32} {(2^40 : Int).toInt16}",
    s!"{(5 : USize) - 7 == 0 - 2} {(3 : USize).toNat} {decide (USize.size > 0)}" ]

def factorial : Nat → Nat
  | 0 => 1
  | n + 1 => (n + 1) * factorial n

def fib (n : Nat) : Nat := Id.run do
  let mut a := 0
  let mut b := 1
  for _ in [0:n] do
    (a, b) := (b, a + b)
  return a

def main : IO Unit := do
  for (a, b) in natCases do IO.println (showNat a b)
  for (a, b) in intCases do IO.println (showInt a b)
  for l in fixedWidth do IO.println l
  IO.println s!"30! = {factorial 30}"
  IO.println s!"fib 300 = {fib 300}"
  IO.println s!"{(2 : Nat) ^ 200} {(3 : Int) ^ 41} {(-3 : Int) ^ 41}"
  IO.println s!"{(10^40 : Nat).toDigits 16} {Nat.toDigits 2 37} {(255 : Nat).toSuperscriptString}"
  IO.println s!"{"12345678901234567890".toNat?} {"-42".toInt?} {"x".toNat?} {"".toNat?}"
