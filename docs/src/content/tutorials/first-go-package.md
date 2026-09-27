---
title: "Your first Go package built from Lean"
description: "Write Lean definitions and a proof, generate a Go package from them with lungo, and call it from a Go program."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, generate a Go package from them, and call it from a Go program. The same Lean project
is used by the tutorials for [Python](first-python-package.md), [Swift](first-swift-package.md),
[TypeScript](first-typescript-package.md) and [C](first-c-library.md).

## Before we start

We need:

- Go 1.22 or later with cgo, and a C compiler (on Windows, MinGW-w64 GCC)
- [elan](https://github.com/leanprover/elan), Lean's toolchain manager
- the `lungo` command

```sh
elan toolchain install leanprover/lean4:v4.34.1
brew install machinefabric/tap/lungo   # apt, dnf, Windows, cargo: see the lungo command's reference
```

## Write the Lean project

Make a Go module with a Lake project in `lean/`:

```sh
mkdir shapes && cd shapes
go mod init example.com/shapes
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

`lungo.toml` names the Lake project and where the Go package goes:

```toml
project = "lean"

[go]
out = "geometry"
```

```sh
lungo generate
```

The first run builds the Lean project, which checks the proof: change `k * k` to `k` in the
theorem and `lungo generate` fails with Lean's error. It prints `generated go into
geometry`. The directory holds `geometry.go` and the program's C files; add `.lungo/` (lungo's
scratch space) to `.gitignore`. Fetch the support module the package imports:

```sh
go mod tidy
```

## Call it from Go

`main.go`:

```go
package main

import (
	"fmt"
	"log"
	"math/big"

	"example.com/shapes/geometry"
)

func main() {
	r := geometry.Rect{Width: big.NewInt(3), Height: big.NewInt(4)}
	area, err := geometry.Area(r)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("area:", area)

	bigger, err := geometry.Scale(big.NewInt(10), r)
	if err != nil {
		log.Fatal(err)
	}
	fmt.Println("scaled:", bigger.Width, "x", bigger.Height)

	best, err := geometry.Largest([]geometry.Rect{r, bigger})
	if err != nil {
		log.Fatal(err)
	}
	if best.Valid {
		fmt.Println("largest:", best.Value.Width, "x", best.Value.Height)
	}
}
```

```text
$ go run .
area: 12
scaled: 30 x 40
largest: 30 x 40
```

`Nat` became `*big.Int` (it has no upper bound), the structure a Go struct, `Option` a
`lungo.Option`, and every function returns an error besides its result: here it can only be
a `*lungo.MalformedError`, for a negative number, which Lean's `Nat` cannot hold.

## What we did

We generated a Go package from a Lake project whose proofs Lean checked, and called it like
any Go package; it runs Lean's compiled code on the lungo runtime, which the `lungo-go`
module links into the binary. Next:

- [Generated packages](../reference/generated-packages.md): every Lean type in Go
- [How to call host code from Lean](../how-to/call-host-code-from-lean.md)
- [How to distribute generated packages](../how-to/distribute-generated-packages.md)
