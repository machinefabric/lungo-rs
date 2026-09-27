"""The lungo runtime library, loaded once per process into the global symbol namespace, where
generated programs find it."""

import ctypes
import os
import sys

_HERE = os.path.dirname(os.path.abspath(__file__))
RUNTIME_DIR = os.path.join(_HERE, "runtime")


def _library_path():
    if sys.platform == "win32":
        return os.path.join(RUNTIME_DIR, "bin", "lungo.dll")
    if sys.platform == "darwin":
        return os.path.join(RUNTIME_DIR, "lib", "liblungo.dylib")
    return os.path.join(RUNTIME_DIR, "lib", "liblungo.so")


def _load():
    path = _library_path()
    if not os.path.isfile(path):
        raise ImportError(f"lungo_py has no runtime library for this platform (expected {path})")
    if sys.platform == "win32":
        # Generated programs link lungo.dll: it is found in this directory.
        os.add_dll_directory(os.path.dirname(path))
        return ctypes.CDLL(path)
    return ctypes.CDLL(path, mode=ctypes.RTLD_GLOBAL)


lib = _load()


class Buffer(ctypes.Structure):
    """`lungo_buffer`: bytes the runtime owns."""

    _fields_ = [
        ("data", ctypes.POINTER(ctypes.c_uint8)),
        ("len", ctypes.c_size_t),
        ("capacity", ctypes.c_size_t),
    ]


DISPATCH = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_uint64, ctypes.POINTER(ctypes.c_uint8), ctypes.c_size_t, ctypes.POINTER(Buffer))
REF = ctypes.CFUNCTYPE(None, ctypes.c_uint64)
ENTRY = ctypes.CFUNCTYPE(ctypes.c_int32, ctypes.c_char_p, ctypes.c_size_t, ctypes.POINTER(Buffer))

lib.lungo_set_host.argtypes = [DISPATCH, REF, REF]
lib.lungo_set_host.restype = None
lib.lungo_handle_release.argtypes = [ctypes.c_uint64]
lib.lungo_handle_release.restype = None
lib.lungo_handle_clone.argtypes = [ctypes.c_uint64]
lib.lungo_handle_clone.restype = ctypes.c_uint64
lib.lungo_buffer_alloc.argtypes = [ctypes.POINTER(Buffer), ctypes.c_size_t]
lib.lungo_buffer_alloc.restype = ctypes.POINTER(ctypes.c_uint8)
lib.lungo_buffer_free.argtypes = [ctypes.POINTER(Buffer)]
lib.lungo_buffer_free.restype = None
lib.lungo_closure_call.argtypes = [
    ctypes.c_void_p,
    ctypes.c_uint64,
    ctypes.c_char_p,
    ctypes.c_size_t,
    ctypes.c_char_p,
    ctypes.c_size_t,
    ctypes.POINTER(Buffer),
]
lib.lungo_closure_call.restype = ctypes.c_int32

ABI_VERSION = ctypes.c_uint32.in_dll(lib, "lungo_abi_v1").value


def take(buf):
    """The bytes of a runtime buffer, which is freed."""
    data = ctypes.string_at(buf.data, buf.len) if buf.len else b""
    lib.lungo_buffer_free(ctypes.byref(buf))
    return data
