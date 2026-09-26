/-! Floating point arithmetic, conversion, and formatting. -/

def samples : List Float :=
  [0.0, -0.0, 1.0, -1.5, 0.1, 1e-310, 1e21, 123456789.125, 3.141592653589793, 2.0 / 0.0, -1.0 / 0.0,
   0.0 / 0.0, 1e308 * 10, 5e-324, 0.5, 2.5, -2.5]

def main : IO Unit := do
  for x in samples do
    IO.println s!"{x} {x.toUInt8} {x.toUInt64} {x.toInt64} {x.floor} {x.ceil} {x.round} {x.abs} {x.isNaN} {x.isInf} {x.isFinite} {x.toBits}"
  for x in [0.5, 1.0, 2.0, 10.0] do
    IO.println s!"{x.sqrt} {x.exp} {x.log} {x.log2} {x.log10} {x.sin} {x.cos} {x.tan} {x.atan} {x.pow 1.5} {x.cbrt} {Float.atan2 x 3.0}"
  IO.println s!"{(0.1 + 0.2 : Float)} {(1.0 : Float) / 3.0} {Float.ofNat 12345678901234567890} {Float.ofInt (-5)}"
  IO.println s!"{(2.5 : Float).toString} {Float.ofScientific 12345 true 2} {(1e100 : Float).toUSize}"
  IO.println s!"{(3.7 : Float).frExp} {Float.scaleB 1.5 10} {decide ((1.0 : Float) < 2.0)} {(0.0/0.0 : Float) == (0.0/0.0)}"
  let f32 : Float32 := 1.1
  IO.println s!"{f32} {f32 * 3} {f32.toFloat} {(0.1 : Float).toFloat32} {f32.sqrt} {f32.toUInt8}"
  IO.println ((List.range 10).map (fun i => Float.ofNat i / 7.0) |>.foldl (· + ·) 0)
