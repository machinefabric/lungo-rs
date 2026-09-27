"""Type descriptors: how Python values cross the boundary as values of Lean types.

A `Type` has the type's expression (for type arguments and function types) and its wire
encoding. Polymorphic functions of generated packages take a `Type` for each type parameter.
"""

import struct
import weakref
from dataclasses import dataclass
from typing import Callable, Generic, TypeVar

from . import _runtime
from ._errors import MalformedError

T = TypeVar("T")
E = TypeVar("E")

# Tags of type expressions.
_NAT, _INT, _BOOL, _UINT8, _UINT16, _UINT32, _UINT64, _USIZE = range(8)
_INT8, _INT16, _INT32, _INT64, _ISIZE, _FLOAT, _FLOAT32, _CHAR = range(8, 16)
_STRING, _UNIT, _BYTE_ARRAY, _FLOAT_ARRAY, _OPTION, _LIST, _ARRAY, _PROD = range(16, 24)
_EXCEPT, _FUNCTION, _INDUCTIVE, _OPAQUE = 24, 25, 27, 28

MAX_FUNCTION_PARAMS = 15


class Type(Generic[T]):
    """A Lean type as Python values of `T`."""

    def expr(self) -> bytes:
        raise NotImplementedError

    def encode(self, w, v: T) -> None:
        raise NotImplementedError

    def decode(self, r) -> T:
        raise NotImplementedError

    def can_be_none(self) -> bool:
        """Whether `None` is a value of the type (it then needs `Some` inside an `Option`)."""
        return False


def _check_int(v, lo, hi, name):
    if not isinstance(v, int) or isinstance(v, bool) or not lo <= v <= hi:
        raise MalformedError(f"{v!r} is not a {name}")
    return v


class _Fixed(Type[int]):
    def __init__(self, tag, fmt, lo, hi, name):
        self.tag, self.fmt, self.lo, self.hi, self.name = tag, fmt, lo, hi, name

    def expr(self):
        return bytes([self.tag])

    def encode(self, w, v):
        w.buf += struct.pack(self.fmt, _check_int(v, self.lo, self.hi, self.name))

    def decode(self, r):
        return struct.unpack(self.fmt, r.take(struct.calcsize(self.fmt)))[0]


def _put_magnitude(w, m):
    w.blob(m.to_bytes((m.bit_length() + 7) // 8, "little"))


def _magnitude(r):
    b = r.blob()
    if b and b[-1] == 0:
        raise MalformedError("a number's magnitude has a leading zero byte")
    return int.from_bytes(b, "little")


class _Nat(Type[int]):
    def expr(self):
        return bytes([_NAT])

    def encode(self, w, v):
        if not isinstance(v, int) or isinstance(v, bool) or v < 0:
            raise MalformedError(f"{v!r} is not a Nat")
        _put_magnitude(w, v)

    def decode(self, r):
        return _magnitude(r)


class _Int(Type[int]):
    def expr(self):
        return bytes([_INT])

    def encode(self, w, v):
        if not isinstance(v, int) or isinstance(v, bool):
            raise MalformedError(f"{v!r} is not an Int")
        w.u8(1 if v < 0 else 0)
        _put_magnitude(w, abs(v))

    def decode(self, r):
        sign = r.u8()
        m = _magnitude(r)
        if sign == 0:
            return m
        if sign == 1 and m == 0:
            raise MalformedError("negative zero is not a canonical Int")
        if sign == 1:
            return -m
        raise MalformedError(f"invalid Int sign {sign}")


class _Bool(Type[bool]):
    def expr(self):
        return bytes([_BOOL])

    def encode(self, w, v):
        if not isinstance(v, bool):
            raise MalformedError(f"{v!r} is not a Bool")
        w.u8(1 if v else 0)

    def decode(self, r):
        b = r.u8()
        if b > 1:
            raise MalformedError(f"invalid Bool {b}")
        return b == 1


class _Float(Type[float]):
    def __init__(self, tag, fmt):
        self.tag, self.fmt = tag, fmt

    def expr(self):
        return bytes([self.tag])

    def encode(self, w, v):
        if not isinstance(v, (float, int)) or isinstance(v, bool):
            raise MalformedError(f"{v!r} is not a float")
        w.buf += struct.pack(self.fmt, v)

    def decode(self, r):
        return struct.unpack(self.fmt, r.take(struct.calcsize(self.fmt)))[0]


def _scalar_value(c):
    return 0 <= c <= 0x10FFFF and not 0xD800 <= c <= 0xDFFF


class _Char(Type[str]):
    def expr(self):
        return bytes([_CHAR])

    def encode(self, w, v):
        if not isinstance(v, str) or len(v) != 1 or not _scalar_value(ord(v)):
            raise MalformedError(f"{v!r} is not a Char")
        w.u32(ord(v))

    def decode(self, r):
        c = r.u32()
        if not _scalar_value(c):
            raise MalformedError(f"{c:#x} is not a Unicode scalar value")
        return chr(c)


class _String(Type[str]):
    def expr(self):
        return bytes([_STRING])

    def encode(self, w, v):
        if not isinstance(v, str):
            raise MalformedError(f"{v!r} is not a String")
        try:
            w.blob(v.encode("utf-8"))
        except UnicodeEncodeError:
            raise MalformedError("a string with lone surrogates is not a String") from None

    def decode(self, r):
        return r.text()


class _Unit(Type[None]):
    def expr(self):
        return bytes([_UNIT])

    def encode(self, w, v):
        if v is not None:
            raise MalformedError(f"{v!r} is not Unit (None)")

    def decode(self, r):
        return None

    def can_be_none(self):
        return True


class _ByteArray(Type[bytes]):
    def expr(self):
        return bytes([_BYTE_ARRAY])

    def encode(self, w, v):
        if not isinstance(v, (bytes, bytearray)):
            raise MalformedError(f"{v!r} is not a ByteArray (bytes)")
        w.blob(bytes(v))

    def decode(self, r):
        return r.blob()


class _FloatArray(Type[list]):
    def expr(self):
        return bytes([_FLOAT_ARRAY])

    def encode(self, w, v):
        if not isinstance(v, (list, tuple)):
            raise MalformedError(f"{v!r} is not a FloatArray (list of float)")
        w.length(len(v))
        for x in v:
            FLOAT.encode(w, x)

    def decode(self, r):
        n = r.count(8)
        return [r.f64() for _ in range(n)]


class Opaque:
    """A Lean value Python does not represent, held by handle: alive until closed or collected."""

    __slots__ = ("_id", "_finalizer", "__weakref__")

    def __init__(self, handle_id):
        self._id = handle_id
        self._finalizer = weakref.finalize(self, _runtime.lib.lungo_handle_release, handle_id)

    def _live(self):
        if not self._finalizer.alive:
            raise MalformedError("the Lean value was closed")
        return self._id

    def close(self):
        """Releases the Lean value."""
        self._finalizer()

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.close()


class _Opaque(Type[Opaque]):
    def expr(self):
        return bytes([_OPAQUE])

    def encode(self, w, v):
        if not isinstance(v, Opaque):
            raise MalformedError(f"{v!r} is not an opaque Lean value")
        live = v._live()
        w.u64(_runtime.lib.lungo_handle_clone(live) if w.result else live)

    def decode(self, r):
        return Opaque(r.u64())


NAT: Type[int] = _Nat()
INT: Type[int] = _Int()
BOOL: Type[bool] = _Bool()
UINT8: Type[int] = _Fixed(_UINT8, "<B", 0, 2**8 - 1, "UInt8")
UINT16: Type[int] = _Fixed(_UINT16, "<H", 0, 2**16 - 1, "UInt16")
UINT32: Type[int] = _Fixed(_UINT32, "<I", 0, 2**32 - 1, "UInt32")
UINT64: Type[int] = _Fixed(_UINT64, "<Q", 0, 2**64 - 1, "UInt64")
USIZE: Type[int] = _Fixed(_USIZE, "<Q", 0, 2**64 - 1, "USize")
INT8: Type[int] = _Fixed(_INT8, "<b", -(2**7), 2**7 - 1, "Int8")
INT16: Type[int] = _Fixed(_INT16, "<h", -(2**15), 2**15 - 1, "Int16")
INT32: Type[int] = _Fixed(_INT32, "<i", -(2**31), 2**31 - 1, "Int32")
INT64: Type[int] = _Fixed(_INT64, "<q", -(2**63), 2**63 - 1, "Int64")
ISIZE: Type[int] = _Fixed(_ISIZE, "<q", -(2**63), 2**63 - 1, "ISize")
FLOAT: Type[float] = _Float(_FLOAT, "<d")
FLOAT32: Type[float] = _Float(_FLOAT32, "<f")
CHAR: Type[str] = _Char()
STRING: Type[str] = _String()
UNIT: Type[None] = _Unit()
BYTE_ARRAY: Type[bytes] = _ByteArray()
FLOAT_ARRAY: Type[list] = _FloatArray()
OPAQUE: Type[Opaque] = _Opaque()


@dataclass(frozen=True)
class Some(Generic[T]):
    """`some value` of an `Option` whose values can be `None` themselves (`Option (Option α)`,
    `Option Unit`); other options are `value` or `None`."""

    value: T


class _Option(Type):
    def __init__(self, t):
        self.t = t

    def expr(self):
        return bytes([_OPTION]) + self.t.expr()

    def can_be_none(self):
        return True

    def encode(self, w, v):
        if v is None:
            w.u8(0)
            return
        if self.t.can_be_none():
            if not isinstance(v, Some):
                raise MalformedError(f"{v!r} is not None or Some(…)")
            v = v.value
        w.u8(1)
        self.t.encode(w, v)

    def decode(self, r):
        tag = r.u8()
        if tag == 0:
            return None
        if tag == 1:
            v = self.t.decode(r)
            return Some(v) if self.t.can_be_none() else v
        raise MalformedError(f"invalid Option tag {tag}")


def option(t: Type[T]) -> Type:
    """`Option α`: `None` or the value (`Some(value)` when the value can be `None`)."""
    return _Option(t)


class _Seq(Type[list]):
    def __init__(self, tag, t):
        self.tag, self.t = tag, t

    def expr(self):
        return bytes([self.tag]) + self.t.expr()

    def encode(self, w, v):
        if not isinstance(v, (list, tuple)):
            raise MalformedError(f"{v!r} is not a list")
        w.length(len(v))
        for x in v:
            self.t.encode(w, x)

    def decode(self, r):
        n = r.count(1)
        return [self.t.decode(r) for _ in range(n)]


def list_of(t: Type[T]) -> Type[list]:
    """`List α`, as a list."""
    return _Seq(_LIST, t)


def array_of(t: Type[T]) -> Type[list]:
    """`Array α`, as a list."""
    return _Seq(_ARRAY, t)


class _Pair(Type[tuple]):
    def __init__(self, a, b):
        self.a, self.b = a, b

    def expr(self):
        return bytes([_PROD]) + self.a.expr() + self.b.expr()

    def encode(self, w, v):
        if not isinstance(v, tuple) or len(v) != 2:
            raise MalformedError(f"{v!r} is not a pair")
        self.a.encode(w, v[0])
        self.b.encode(w, v[1])

    def decode(self, r):
        return (self.a.decode(r), self.b.decode(r))


def pair(a: Type, b: Type) -> Type[tuple]:
    """`α × β`, as a tuple."""
    return _Pair(a, b)


@dataclass(frozen=True)
class Ok(Generic[T]):
    """`Except.ok value`."""

    value: T


@dataclass(frozen=True)
class Err(Generic[E]):
    """`Except.error error`."""

    error: E


class _Except(Type):
    def __init__(self, e, a):
        self.e, self.a = e, a

    def expr(self):
        return bytes([_EXCEPT]) + self.e.expr() + self.a.expr()

    def encode(self, w, v):
        if isinstance(v, Ok):
            w.u8(1)
            self.a.encode(w, v.value)
        elif isinstance(v, Err):
            w.u8(0)
            self.e.encode(w, v.error)
        else:
            raise MalformedError(f"{v!r} is not Ok(…) or Err(…)")

    def decode(self, r):
        tag = r.u8()
        if tag == 0:
            return Err(self.e.decode(r))
        if tag == 1:
            return Ok(self.a.decode(r))
        raise MalformedError(f"invalid Except tag {tag}")


def except_(e: Type, a: Type) -> Type:
    """`Except ε α`, as `Ok(value)` or `Err(error)`."""
    return _Except(e, a)


def inductive_expr(index: int, *args: bytes) -> bytes:
    """The type expression of type `index` of a program's type table applied to `args`."""
    return struct.pack("<BII", _INDUCTIVE, index, len(args)) + b"".join(args)


class LeanFunction:
    """A Lean function: callable with its arguments; fails only with MalformedError."""

    __slots__ = ("_opaque", "_program", "_type")

    def __init__(self, handle_id, program, fn_type):
        self._opaque = Opaque(handle_id)
        self._program = program
        self._type = fn_type

    def __call__(self, *args):
        return self._program.call_closure(self._opaque._live(), self._type, args)

    def close(self):
        self._opaque.close()


class _Function(Type[Callable]):
    def __init__(self, params, result):
        if not 1 <= len(params) <= MAX_FUNCTION_PARAMS:
            raise ValueError(f"a function type of {len(params)} parameters")
        self.params, self.result = list(params), result

    def expr(self):
        return struct.pack("<BI", _FUNCTION, len(self.params)) + b"".join(p.expr() for p in self.params) + self.result.expr()

    def encode(self, w, v):
        if not callable(v):
            raise MalformedError(f"{v!r} is not callable")
        if isinstance(v, LeanFunction):
            w.u8(0)
            live = v._opaque._live()
            w.u64(_runtime.lib.lungo_handle_clone(live) if w.result else live)
            return
        from . import _program

        w.u8(1)
        w.u64(_program.host_function(w, self, v))

    def decode(self, r):
        kind = r.u8()
        handle_id = r.u64()
        if kind != 0:
            raise MalformedError(f"the runtime sent a function of kind {kind}")
        return LeanFunction(handle_id, r.program, self)


def function(params, result: Type) -> Type[Callable]:
    """The type of Lean functions from `params` to `result`, as Python callables. A Python
    function passed to Lean may run on any thread; an exception it raises terminates the Lean
    program, since Lean's functions cannot fail."""
    return _Function(params, result)

