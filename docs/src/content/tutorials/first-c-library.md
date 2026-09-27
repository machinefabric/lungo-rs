---
title: "Your first C library built from Lean"
description: "Write Lean definitions and a proof, generate a C library from them with lungo, and call it from a C program built with CMake."
---

In this tutorial we will write a few Lean definitions about rectangles, prove a fact about
them, generate a C library from them with lungo, and use it. The Lean project is the same as in
the tutorials for [Go](first-go-package.md), [Python](first-python-package.md), [Swift](first-swift-package.md) and [TypeScript](first-typescript-package.md).

## Before we start

We need:

- CMake 3.20 or later and a C11 compiler
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

## Generate the library

`lungo.toml` names the Lake project and where the C library goes:

```toml
project = "lean"

[c]
out = "geometry-c"
```

```sh
lungo generate
```

The first run builds the Lean project, which checks the proof: change `k * k` to `k` in the
theorem and `lungo generate` fails with Lean's error. `geometry-c` is a CMake project of the
library `geometry`, whose API is `include/geometry.h`; add `.lungo/` (lungo's scratch space)
to `.gitignore`.

## Call it from C

`CMakeLists.txt` of the program:

```cmake
cmake_minimum_required(VERSION 3.20)
project(shapes C)
add_subdirectory(geometry-c)
add_executable(shapes main.c)
target_link_libraries(shapes PRIVATE geometry)
```

`main.c`:

```c
#include <stdio.h>
#include <stdlib.h>

#include "geometry.h"

/* Exits with the error of a failed call. */
static lungo_value *check(int32_t status, lungo_value *result, lungo_error *error) {
    if (status != LUNGO_OK) {
        fprintf(stderr, "error: %s\n", lungo_error_message(error));
        exit(1);
    }
    return result;
}

static void print_rect(const char *label, const lungo_value *r) {
    char *w = lungo_value_number_string(geometry_rect_width(r));
    char *h = lungo_value_number_string(geometry_rect_height(r));
    printf("%s: %s x %s\n", label, w, h);
    lungo_string_free(w);
    lungo_string_free(h);
}

int main(void) {
    lungo_value *result = NULL;
    lungo_error *error = NULL;

    lungo_value *r = geometry_rect_mk(lungo_value_nat(3), lungo_value_nat(4));
    lungo_value *area = check(geometry_area(r, &result, &error), result, error);
    char *digits = lungo_value_number_string(area);
    printf("area: %s\n", digits);
    lungo_string_free(digits);

    lungo_value *ten = lungo_value_nat(10);
    lungo_value *bigger = check(geometry_scale(ten, r, &result, &error), result, error);
    print_rect("scaled", bigger);

    lungo_value *items[2] = {lungo_value_clone(r), lungo_value_clone(bigger)};
    lungo_value *list = lungo_value_list(items, 2);
    lungo_value *best = check(geometry_largest(list, &result, &error), result, error);
    if (lungo_value_get_option(best)) print_rect("largest", lungo_value_get_option(best));

    lungo_value_free(best);
    lungo_value_free(list);
    lungo_value_free(bigger);
    lungo_value_free(ten);
    lungo_value_free(area);
    lungo_value_free(r);
    return 0;
}
```

```text
$ cmake -S . -B build && cmake --build build && ./build/shapes
area: 12
scaled: 30 x 40
largest: 30 x 40
```

CMake downloads the lungo runtime for the platform and checks its SHA-256 digest before
linking it. Every Lean value is a `lungo_value`: constructors (`geometry_rect_mk`) take
ownership of their parts, accessors (`geometry_rect_width`) borrow, and each value the API
returns is freed once.

## What we did

We generated a C library from a Lake project whose proofs Lean checked; it runs Lean's
compiled code on the lungo runtime. Next:

- [The C API](../reference/c-api.md)
- [Generated packages](../reference/generated-packages.md)
- [How to call host code from Lean](../how-to/call-host-code-from-lean.md)
