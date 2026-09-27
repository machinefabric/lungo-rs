---
title: "Your first Python package built from Lean"
description: "Write Lean definitions and a proof, generate a Python package from them with lungo, and call it from Python."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, generate a Python package from them with lungo, and use it. The Lean project is the same as in
the tutorials for [Go](first-go-package.md), [Swift](first-swift-package.md), [TypeScript](first-typescript-package.md) and [C](first-c-library.md).

## Before we start

We need:

- Python 3.9 or later, CMake and a C compiler
- [elan](https://github.com/leanprover/elan), Lean's toolchain manager
- the `lungo` command

```sh
elan toolchain install leanprover/lean4:v4.34.1
curl -sSfL https://github.com/machinefabric/lungo/releases/latest/download/install.sh | sh
```

## Write the Lean project

Make a directory with a Lake project in `lean/`:

```sh
mkdir shapes && cd shapes
mkdir lean
echo 'leanprover/lean4:v4.34.1' > lean/lean-toolchain
```

`lean/lakefile.toml` declares a library whose root module Lake builds by default:

```toml
name = "geometry"
version = "0.1.0"
defaultTargets = ["Geometry"]

[[lean_lib]]
name = "Geometry"
```

`lean/Geometry.lean` holds the definitions and the proof:

```lean
namespace Geometry

structure Rect where
  width : Nat
  height : Nat
deriving Repr

def area (r : Rect) : Nat := r.width * r.height

def scale (k : Nat) (r : Rect) : Rect :=
  { width := k * r.width, height := k * r.height }

/-- Scaling by `k` multiplies the area by `k * k`. -/
theorem area_scale (k : Nat) (r : Rect) : area (scale k r) = k * k * area r := by
  simp only [area, scale]
  exact Nat.mul_mul_mul_comm k r.width k r.height

/-- The rectangle with the largest area, if any. -/
def largest (rs : List Rect) : Option Rect :=
  rs.foldl (fun best r => match best with
    | some b => if area r > area b then some r else some b
    | none => some r) none

end Geometry
```

Create Lake's manifest once (it is part of the project):

```sh
(cd lean && lake update)
```

## Generate the package

`lungo.toml` names the Lake project and where the Python project goes:

```toml
project = "lean"

[python]
out = "geometry-py"
```

```sh
lungo generate
```

The first run builds the Lean project, which checks the proof: change `k * k` to `k` in the
theorem and `lungo generate` fails with Lean's error. `geometry-py` is a Python project; add
`.lungo/` (lungo's scratch space) to `.gitignore`. Install it into a virtual environment; it
brings `lungo-py`, which carries the runtime:

```sh
python3 -m venv .venv
.venv/bin/pip install ./geometry-py
```

## Call it from Python

`shapes.py`:

```python
from geometry import Rect, area, largest, scale

r = Rect(width=3, height=4)
print("area:", area(r))
bigger = scale(10, r)
print("scaled:", bigger)
print("largest:", largest([r, bigger]))
```

```text
$ .venv/bin/python shapes.py
area: 12
scaled: Rect(width=30, height=40)
largest: Rect(width=30, height=40)
```

The structure became a frozen dataclass, `Nat` an `int` (a negative one raises
`lungo_py.MalformedError`), and `Option` a value or `None`.

## What we did

We generated a Python package from a Lake project whose proofs Lean checked; its functions
run Lean's compiled code on the lungo runtime. Next:

- [Generated packages](../reference/generated-packages.md): every Lean type in Python
- [How to call host code from Lean](../how-to/call-host-code-from-lean.md)
- [How to distribute generated packages](../how-to/distribute-generated-packages.md)
