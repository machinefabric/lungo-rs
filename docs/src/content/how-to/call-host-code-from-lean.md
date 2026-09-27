---
title: "How to call host code from Lean"
description: "Declare a Lean extern and implement it in Go, Python, Swift, TypeScript or C."
---

This guide shows how Lean code calls a function of the application, in the language of the
generated package. For Rust, see [how to call Rust functions from Lean](call-rust-from-lean.md).

## Declare the extern in Lean

```lean
namespace Store

/-- Looks a key up in the application's store. -/
@[extern "store_lookup"]
opaque lookup (key : @& String) : Option Nat

/-- Appends a line to the application's log. -/
@[extern "store_log"]
opaque log (line : String) : IO Unit

def total (keys : List String) : IO Nat := do
  let mut sum := 0
  for k in keys do
    match lookup k with
    | some v => log s!"{k} = {v}"; sum := sum + v
    | none => throw (IO.userError s!"unknown key {k}")
  return sum

end Store
```

## List it as a host extern

```toml
# lungo.toml
[lean]
host-externs = ["store_lookup", "store_log"]
```

(or `--host-extern store_lookup` on the command line). An extern whose symbol the runtime or
the program implements is not a host extern; one that nothing implements is an error
([`LNG0401`](../reference/errors.md#lng0401)).

## Implement it

The generated package has one method (or function) per host extern, named after the Lean
declaration, with the declaration's types. An `IO` extern fails with the language's IO error
(any other error becomes `IO.userError` with its message); an extern that is not `IO` or
`EIO` must not fail: its failure terminates the program. Install the implementation before
the program's first call.

### Go

```go
type store struct{ values map[string]*big.Int }

func (s *store) Lookup(key string) (lungo.Option[*big.Int], error) {
	v, ok := s.values[key]
	return lungo.Option[*big.Int]{Value: v, Valid: ok}, nil
}

func (s *store) Log(line string) error {
	log.Println(line)
	return nil
}

store.SetHost(&store{values: map[string]*big.Int{"a": big.NewInt(1)}})
```

### Python

```python
class Store:
    def lookup(self, key: str):
        return {"a": 1}.get(key)

    def log(self, line: str) -> None:
        print(line)

store.set_host(Store())
```

### Swift

```swift
final class Store: StoreHost {
    func lookup(_ key: String) throws -> LungoNat? { key == "a" ? 1 : nil }
    func log(_ line: String) throws { print(line) }
}

setHost(Store())
```

### TypeScript

```ts
const p = await load({
  host: {
    lookup: (key) => (key === "a" ? 1n : null),
    log: (line) => console.log(line),
  },
});
```

### C

```c
static int32_t lookup(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    size_t len;
    const char *key = lungo_value_get_string(args[0], &len);
    *result = len == 1 && key[0] == 'a' ? lungo_value_some(lungo_value_nat(1)) : lungo_value_none();
    return LUNGO_OK;
}

static int32_t log_line(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error) {
    size_t len;
    const char *line = lungo_value_get_string(args[0], &len);
    printf("%.*s\n", (int)len, line);
    *result = lungo_value_unit();
    return LUNGO_OK;
}

store_implement_lookup(lookup, NULL, NULL);
store_implement_log(log_line, NULL, NULL);
```

## Pass functions both ways

A Lean parameter of function type accepts a function of the host language, which Lean may
call any number of times, on any thread, while it holds it; a Lean function returned to the
host is a function the host calls. Both cross as described in
[the wire format](../reference/wire-format.md#host-functions); nothing needs declaring.
