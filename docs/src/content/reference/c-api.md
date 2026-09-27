---
title: "The C API (lungo.h)"
description: "The lungo runtime's C interface: the value API C and Objective-C programs use, and the program API generated code uses."
---

`lungo.h` is the interface of the prebuilt runtime library (`liblungo`). It is included in
every runtime package (`include/lungo.h`) and every generated package. Its C ABI has a
version, `LUNGO_ABI_VERSION`; generated code calls the function `lungo_abi_v<N>`, which only
a runtime of that ABI defines, so it cannot link with a runtime of another ABI.

The header has three parts: the **value API**, for C and Objective-C programs using a
generated C package; the **program API** and the **boundary**, for generated code and the
support libraries; and the runtime's primitives (`lungo_<symbol>` for every Lean
`@[extern]` symbol the runtime implements), for generated code.

## Values

A `lungo_value *` is a Lean value as a tree of plain data. The API returns owned values,
freed with `lungo_value_free`; constructors of composite values take ownership of their
parts; accessors return pointers borrowed from their argument. A value owns the handles and
host functions in it: freeing it releases them, `lungo_value_clone` duplicates them.

| Kind | Construct | Read |
| --- | --- | --- |
| `LUNGO_NAT` | `lungo_value_nat(uint64_t)`, `lungo_value_nat_parse(const char *digits)` | `lungo_value_get_nat(v, &u64)` (false if it does not fit), `lungo_value_number_string(v)` |
| `LUNGO_INT` | `lungo_value_int(int64_t)`, `lungo_value_int_parse(const char *digits)` | `lungo_value_get_int(v, &i64)`, `lungo_value_number_string(v)` |
| `LUNGO_BOOL`, `LUNGO_UINT8` … `LUNGO_ISIZE`, `LUNGO_FLOAT`, `LUNGO_FLOAT32` | `lungo_value_bool(b)`, `lungo_value_uint8(x)`, … | `lungo_value_get_bool(v)`, `lungo_value_get_uint8(v)`, … |
| `LUNGO_CHAR` | `lungo_value_char(uint32_t)` (`NULL` if not a Unicode scalar value) | `lungo_value_get_char(v)` |
| `LUNGO_STRING` | `lungo_value_string(bytes, len)`, `lungo_value_cstring(s)` (`NULL` if not UTF-8) | `lungo_value_get_string(v, &len)` (not NUL-terminated), `lungo_value_string_dup(v)` |
| `LUNGO_UNIT` | `lungo_value_unit()` | |
| `LUNGO_BYTE_ARRAY`, `LUNGO_FLOAT_ARRAY` | `lungo_value_byte_array(data, len)`, `lungo_value_float_array(data, len)` | `lungo_value_get_bytes(v, &len)`, `lungo_value_get_floats(v, &len)` |
| `LUNGO_OPTION` | `lungo_value_none()`, `lungo_value_some(x)` | `lungo_value_get_option(v)` (`NULL` for `none`) |
| `LUNGO_LIST`, `LUNGO_ARRAY` | `lungo_value_list(items, n)`, `lungo_value_array(items, n)` | `lungo_value_count(v)`, `lungo_value_item(v, i)` |
| `LUNGO_PROD` | `lungo_value_prod(a, b)` | `lungo_value_first(v)`, `lungo_value_second(v)` |
| `LUNGO_EXCEPT` | `lungo_value_ok(x)`, `lungo_value_error(e)` | `lungo_value_is_ok(v)`, `lungo_value_get_except(v)` |
| `LUNGO_FUNCTION` | `lungo_value_function(fn_type, f, ctx, drop)` | `lungo_value_call(f, fn_type, args, n, &result, &error)` |
| `LUNGO_INDUCTIVE` | `lungo_value_ctor(index, fields, n)` (generated constructors call it) | `lungo_value_ctor_index(v)`, `lungo_value_field_count(v)`, `lungo_value_field(v, i)` |
| `LUNGO_OPAQUE` | (received from Lean) | |

`lungo_value_kind(v)` is the value's kind; `lungo_value_equal(a, b)` compares values
(opaque values and functions are equal when they are the same handle or host function);
strings the API returns are freed with `lungo_string_free`.

Misusing the API (a null pointer, reading a value of another kind, an index out of range)
terminates the process with a message naming the function. Invalid data (a string that is
not UTF-8, digits that are not a number) is reported by a `NULL` result.

## Types

A `lungo_type *` is a type expression: `lungo_type_simple(LUNGO_NAT)` (and the other kinds
without parts), `lungo_type_option(t)`, `lungo_type_list(t)`, `lungo_type_array(t)`,
`lungo_type_prod(a, b)`, `lungo_type_except(e, a)`, `lungo_type_function(params, n, result)`,
and the named types of a program from its generated descriptors. Constructors borrow their
arguments; types are freed with `lungo_type_free`. Polymorphic functions take the types of
their type arguments; function values need their type to be called.

## Calls and errors

A generated function returns a status:

| Status | Meaning |
| --- | --- |
| `LUNGO_OK` | `*result` holds the result (owned). |
| `LUNGO_FAILED` | The function failed: `*error` holds an `IO.Error` (`LUNGO_ERROR_IO`) or an `EIO` error value (`LUNGO_ERROR_VALUE`, `lungo_error_value(e)`). |
| `LUNGO_MALFORMED` | The arguments do not match the function's parameters (`LUNGO_ERROR_MALFORMED`). |

`lungo_error_kind(e)`, `lungo_error_message(e)` (for display; the value itself for a string
error value) and `lungo_error_value(e)` read an error; `lungo_error_free(e)` frees it.

## Host functions

A host function has the type `lungo_function`:

```c
int32_t f(void *ctx, const lungo_value *const *args, size_t n, lungo_value **result, lungo_error **error);
```

It returns `LUNGO_OK` with its result (owned by the runtime afterwards), or `LUNGO_FAILED`
with an error: `lungo_error_io(message)` for an `IO` extern (Lean's `IO.userError`; a copy of
an error received from Lean, `lungo_error_clone(e)`, raises that error), or
`lungo_error_from_value(v)` for an `EIO` extern. An error of a host function whose Lean type
is neither is fatal to the program. Host functions may be called on any thread. The `drop`
function given with one frees its context once no value or Lean closure refers to it.

The first host function created makes the C API the process's host; a process has one host,
so the C API's host functions cannot be combined with another language's support library.

## Program API and boundary

These parts are used by generated code and the support libraries; they are documented in
`lungo.h` itself:

- objects: `lungo_obj`, boxing, reference counting fast paths (inline) and slow paths,
  allocation, closures and application;
- constants and initializers (`lungo_lazy_obj`, `lungo_init_obj`, …), the program's
  initialization and `lungo_run_main`;
- the boundary of the [wire format](wire-format.md): `lungo_types_load`, the call context
  (`lungo_call_*`), host calls (`lungo_hostcall_*`), `lungo_closure_call`,
  `lungo_set_host`, handles (`lungo_handle_clone`, `lungo_handle_release`) and buffers
  (`lungo_buffer_alloc`, `lungo_buffer_free`);
- `lungo_invoke` and `lungo_host_function_new`, through which generated C packages implement
  their API on the value API.
