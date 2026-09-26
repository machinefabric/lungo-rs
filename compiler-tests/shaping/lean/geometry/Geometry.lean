namespace Geometry

structure Point where
  x : Int
  y : Int

inductive Shape where
  | dot (at_ : Point)
  | segment (start stop : Point)

structure Secret where
  code : Nat

def origin : Point := ⟨0, 0⟩

def translate (p : Point) (dx dy : Int) : Point := ⟨p.x + dx, p.y + dy⟩

def endpoints : Shape → List Point
  | .dot p => [p]
  | .segment a b => [a, b]

def reveal (s : Secret) : Nat := s.code * 2

end Geometry
