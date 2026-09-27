---
title: "Your first TypeScript package built from Lean"
description: "Write Lean definitions and a proof, generate an npm package running on WebAssembly with lungo, and call it from JavaScript."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, generate a TypeScript package (running as WebAssembly) from them with lungo, and use it. The Lean project is the same as in
the tutorials for [Go](first-go-package.md), [Python](first-python-package.md), [Swift](first-swift-package.md) and [C](first-c-library.md).

## Before we start

We need:

- Node.js 20 or later
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

`lungo.toml` names the Lake project and where the npm package goes:

```toml
project = "lean"

[ts]
out = "geometry-js"
```

```sh
lungo generate
```

The first run builds the Lean project, which checks the proof: change `k * k` to `k` in the
theorem and `lungo generate` fails with Lean's error. It also downloads the pinned wasi-sdk
(once; its digest is checked) and links the program to `geometry-js/program.wasm`. Add
`.lungo/` (lungo's scratch space) to `.gitignore`.

## Call it from JavaScript

`package.json` of the application:

```json
{ "type": "module", "dependencies": { "geometry": "file:./geometry-js" } }
```

`shapes.js`:

```js
import { load } from "geometry";

const geometry = await load();
const r = { width: 3n, height: 4n };
console.log("area:", geometry.area(r));
const bigger = geometry.scale(10n, r);
console.log("scaled:", bigger);
console.log("largest:", geometry.largest([r, bigger]));
```

```text
$ npm install && node shapes.js
area: 12n
scaled: { width: 30n, height: 40n }
largest: { width: 30n, height: 40n }
```

`Nat` became `bigint`, the structure a plain object, and `Option` a value or `null`;
`index.d.ts` gives TypeScript the types. Node.js may print an `ExperimentalWarning` about
WASI, which it runs the module on. In a browser, `load()` fetches `program.wasm` next to the
module and runs it on lungo-ts's minimal WASI.

## What we did

We generated an npm package from a Lake project whose proofs Lean checked; its methods run
Lean's compiled code, compiled to WebAssembly with the lungo runtime. Next:

- [Generated packages](../reference/generated-packages.md): every Lean type in TypeScript,
  and what WebAssembly cannot do
- [How to call host code from Lean](../how-to/call-host-code-from-lean.md)
- [How to distribute generated packages](../how-to/distribute-generated-packages.md)
