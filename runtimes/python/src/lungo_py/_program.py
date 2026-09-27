"""Generated programs: calls into Lean, and the host functions Lean calls."""

import ctypes
import threading

from . import _runtime
from ._errors import HostError, LeanError, LeanIOError, MalformedError
from ._types import Opaque
from ._wire import Reader, Writer

_lib = _runtime.lib

# Host functions the runtime may call: identifier -> [program, call(reader, writer), references].
_registry = {}
_registry_lock = threading.Lock()
_next_id = 0


def _register(program, call):
    """Registers `call` with one reference, owned by the caller."""
    global _next_id
    with _registry_lock:
        _next_id += 1
        _registry[_next_id] = [program, call, 1]
        return _next_id


def _retain(callback):
    with _registry_lock:
        entry = _registry.get(callback)
        if entry is None:
            raise HostError(f"the runtime retained host function {callback}, which is not registered")
        entry[2] += 1


def _release(callback):
    with _registry_lock:
        entry = _registry.get(callback)
        if entry is None:
            raise HostError(f"host function {callback} is released more often than it is referenced")
        entry[2] -= 1
        if entry[2] == 0:
            del _registry[callback]


def host_function(w, fn_type, f):
    """Registers the Python function `f` of Lean type `fn_type` for the writer's call: lent for
    the call when the values are arguments, given to the runtime when they are a result."""

    def call(r, out):
        args = [p.decode(r) for p in fn_type.params]
        try:
            result = f(*args)
        except Exception as e:
            raise HostError(f"a Python function passed to Lean failed: {e!r}") from e
        fn_type.result.encode(out, result)

    callback = _register(w.program, call)
    if not w.result:
        w.temps.append(callback)
    return callback


def _dispatch(callback, data, length, out):
    try:
        with _registry_lock:
            entry = _registry.get(callback)
        if entry is None:
            raise HostError(f"the runtime called host function {callback}, which is not registered")
        program, call, _ = entry
        r = Reader(program, ctypes.string_at(data, length) if length else b"")
        w = Writer(program, result=True)
        call(r, w)
        r.finish()
        status, payload = 0, bytes(w.buf)
    except BaseException as e:  # Nothing may unwind into the runtime.
        status, payload = 1, (str(e) or type(e).__name__).encode("utf-8", "replace")
    if payload:
        dst = _lib.lungo_buffer_alloc(out, len(payload))
        ctypes.memmove(dst, payload, len(payload))
    return status


def _retain_entry(callback):
    try:
        _retain(callback)
    except HostError as e:
        _fatal(e)


def _release_entry(callback):
    try:
        _release(callback)
    except HostError as e:
        _fatal(e)


def _fatal(e):
    import os
    import sys

    print(f"lungo_py: {e}", file=sys.stderr, flush=True)
    os.abort()


# The host's entry points, kept alive for the life of the process.
_DISPATCH = _runtime.DISPATCH(_dispatch)
_RETAIN = _runtime.REF(_retain_entry)
_RELEASE = _runtime.REF(_release_entry)
_lib.lungo_set_host(_DISPATCH, _RETAIN, _RELEASE)


class Returns:
    """What a function returns: a value, an `IO` result, or an `EIO ε` result."""

    __slots__ = ("kind", "value", "error")

    def __init__(self, kind, value, error=None):
        self.kind, self.value, self.error = kind, value, error


def value(t):
    return Returns("value", t)


def io(t):
    return Returns("io", t)


def eio(e, t):
    return Returns("eio", t, e)


class Program:
    """A generated program: its shared library and type table. Generated packages create one."""

    def __init__(self, path, types_symbol):
        self._lib = ctypes.CDLL(path)
        types = getattr(self._lib, types_symbol)
        types.restype = ctypes.c_void_p
        types.argtypes = []
        self.types = types()
        self._entries = {}

    def _entry(self, symbol):
        f = self._entries.get(symbol)
        if f is None:
            f = _runtime.ENTRY((symbol, self._lib))
            self._entries[symbol] = f
        return f

    def invoke(self, symbol, type_args, args, returns):
        """Calls the entry point `symbol` with the type arguments `type_args` and the arguments
        `args` (pairs of type and value); the result per `returns`."""
        w = Writer(self)
        try:
            w.u32(len(type_args))
            for t in type_args:
                w.buf += t.expr()
            for t, v in args:
                t.encode(w, v)
            data = bytes(w.buf)
            buf = _runtime.Buffer()
            status = self._entry(symbol)(data, len(data), ctypes.byref(buf))
            out = _runtime.take(buf)
        finally:
            for callback in w.temps:
                _release(callback)
        if status == 1:
            raise MalformedError(out.decode("utf-8", "replace"))
        if status != 0:
            raise HostError(f"a generated entry point returned status {status}")
        return self._result(out, returns)

    def _result(self, out, returns):
        r = Reader(self, out)
        try:
            if returns.kind == "value":
                v = returns.value.decode(r)
                failure = None
            else:
                tag = r.u8()
                failure = None
                if tag == 0:
                    v = returns.value.decode(r)
                elif tag == 1 and returns.kind == "io":
                    handle, message = r.u64(), r.text()
                    failure = LeanIOError(message, Opaque(handle))
                elif tag == 1:
                    failure = LeanError(returns.error.decode(r))
                else:
                    raise MalformedError(f"invalid result tag {tag}")
            r.finish()
        except MalformedError as e:
            raise HostError(f"the runtime produced a malformed result: {e}") from e
        if failure is not None:
            raise failure
        return v

    def call_closure(self, handle_id, fn_type, args):
        if len(args) != len(fn_type.params):
            raise MalformedError(f"{len(args)} arguments for {len(fn_type.params)} parameters")
        w = Writer(self)
        try:
            w.u32(0)
            for t, v in zip(fn_type.params, args):
                t.encode(w, v)
            data = bytes(w.buf)
            expr = fn_type.expr()
            buf = _runtime.Buffer()
            status = _lib.lungo_closure_call(self.types, handle_id, expr, len(expr), data, len(data), ctypes.byref(buf))
            out = _runtime.take(buf)
        finally:
            for callback in w.temps:
                _release(callback)
        if status != 0:
            raise MalformedError(out.decode("utf-8", "replace"))
        return self._result(out, value(fn_type.result))

    def set_host_extern(self, set_symbol, index, params, returns, f):
        """Implements host extern `index` with `f`, through the program's `set_host_extern`."""

        def call(r, w):
            args = [p.decode(r) for p in params]
            if returns.kind == "value":
                try:
                    result = f(*args)
                except Exception as e:
                    raise HostError(f"{e!r}") from e
                returns.value.encode(w, result)
                return
            try:
                result = f(*args)
            except LeanIOError as e:
                if returns.kind != "io":
                    raise HostError(f"an EIO extern raised LeanIOError: {e}") from e
                w.u8(1)
                handle = e._handle
                w.u64(_lib.lungo_handle_clone(handle._live()) if handle is not None else 0)
                w.blob(e.message.encode("utf-8", "replace"))
                return
            except LeanError as e:
                if returns.kind != "eio":
                    raise HostError(f"an IO extern raised LeanError: {e!r}") from e
                w.u8(1)
                returns.error.encode(w, e.value)
                return
            except Exception as e:
                if returns.kind != "io":
                    raise HostError(f"an EIO extern failed without an error value: {e!r}") from e
                # Any other exception of an IO extern is IO.userError with its message.
                w.u8(1)
                w.u64(0)
                w.blob((str(e) or type(e).__name__).encode("utf-8", "replace"))
                return
            w.u8(0)
            returns.value.encode(w, result)

        setter = getattr(self._lib, set_symbol)
        setter.argtypes = [ctypes.c_size_t, ctypes.c_uint64]
        setter.restype = None
        setter(index, _register(self, call))

    def run_main(self, symbol, args):
        """Runs the program's `main` with `args`; its exit code."""
        encoded = []
        for i, a in enumerate(args):
            if not isinstance(a, str) or "\0" in a:
                raise MalformedError(f"argument {i} is not a string without NUL")
            try:
                encoded.append(a.encode("utf-8"))
            except UnicodeEncodeError:
                raise MalformedError(f"argument {i} is not valid Unicode") from None
        argv = (ctypes.c_char_p * max(len(encoded), 1))(*encoded)
        run = getattr(self._lib, symbol)
        run.argtypes = [ctypes.c_size_t, ctypes.POINTER(ctypes.c_char_p)]
        run.restype = ctypes.c_int32
        return run(len(encoded), argv)
