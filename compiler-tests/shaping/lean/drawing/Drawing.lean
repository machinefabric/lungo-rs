import Geometry

namespace Drawing

def midpoint (a b : Geometry.Point) : Geometry.Point :=
  ⟨(a.x + b.x) / 2, (a.y + b.y) / 2⟩

def outline (s : Geometry.Shape) : List Geometry.Point :=
  Geometry.endpoints s ++ [Geometry.origin]

end Drawing
