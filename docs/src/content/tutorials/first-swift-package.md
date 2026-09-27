---
title: "Your first Swift package built from Lean"
description: "Write Lean definitions and a proof, generate a Swift package from them with lungo, and call it from Swift."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, generate a Swift package from them with lungo, and use it. The Lean project is the same as in
the tutorials for [Go](first-go-package.md), [Python](first-python-package.md), [TypeScript](first-typescript-package.md) and [C](first-c-library.md).

## Before we start

We need:

- Xcode or a Swift 5.9 toolchain on a Mac
- [elan](https://github.com/leanprover/elan), Lean's toolchain manager
- the `lungo` command

```sh
elan toolchain install leanprover/lean4:v4.34.1
brew install machinefabric/tap/lungo   # apt, dnf, Windows, cargo: see the lungo command's reference
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

`lungo.toml` names the Lake project and where the Swift package goes:

```toml
project = "lean"

[swift]
out = "geometry-swift"
```

```sh
lungo generate
```

The first run builds the Lean project, which checks the proof: change `k * k` to `k` in the
theorem and `lungo generate` fails with Lean's error. `geometry-swift` is a Swift package
with the product `Geometry`; add `.lungo/` (lungo's scratch space) to `.gitignore`.

## Call it from Swift

A command-line package next to it, `Package.swift`:

```swift
// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Shapes",
    platforms: [.macOS("12.0")],
    dependencies: [.package(path: "geometry-swift")],
    targets: [
        .executableTarget(name: "Shapes", dependencies: [.product(name: "Geometry", package: "geometry-swift")])
    ]
)
```

`Sources/Shapes/main.swift`:

```swift
import Geometry

let r = Rect(width: 3, height: 4)
print("area:", try area(r))
let bigger = try scale(10, r)
print("scaled:", bigger.width, "x", bigger.height)
if let best = try largest([r, bigger]) {
    print("largest:", best.width, "x", best.height)
}
```

```text
$ swift run
area: 12
scaled: 30 x 40
largest: 30 x 40
```

The structure became a Swift struct, `Nat` a `LungoNat` (with integer literals), `Option` an
optional, and every function `throws`. Objective-C code uses the same package through its C
API: `@import GeometryProgram;`.

## What we did

We generated a Swift package from a Lake project whose proofs Lean checked; it runs Lean's
compiled code on the lungo runtime, which `lungo-swift` provides as an XCFramework. Next:

- [Generated packages](../reference/generated-packages.md): every Lean type in Swift
- [How to call host code from Lean](../how-to/call-host-code-from-lean.md)
- [How to distribute generated packages](../how-to/distribute-generated-packages.md)
