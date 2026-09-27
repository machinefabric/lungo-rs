"""lungo_py: the support library of the Python packages lungo generates from Lean programs.

Generated packages call their Lean program through `Program`; their functions take and return
Python values (`int` for Nat and Int, frozen dataclasses for structures and constructors) and
take a `Type` for each type parameter of a polymorphic function. Errors: `LeanIOError` (an
`IO` function failed), `LeanError` (an `EIO ε` function failed, with its error value),
`MalformedError` (arguments Lean cannot represent).
"""

import os

from . import _runtime
from ._errors import HostError, LeanError, LeanIOError, MalformedError
from ._program import Program, eio, io, value
from ._types import (
    BOOL,
    BYTE_ARRAY,
    CHAR,
    FLOAT,
    FLOAT32,
    FLOAT_ARRAY,
    INT,
    INT8,
    INT16,
    INT32,
    INT64,
    ISIZE,
    NAT,
    OPAQUE,
    STRING,
    UINT8,
    UINT16,
    UINT32,
    UINT64,
    UNIT,
    USIZE,
    Err,
    LeanFunction,
    Ok,
    Opaque,
    Some,
    Type,
    array_of,
    except_,
    function,
    inductive_expr,
    list_of,
    option,
    pair,
)
from ._version import __version__

ABI_VERSION = _runtime.ABI_VERSION


def runtime_dir() -> str:
    """The lungo runtime package this library carries (`include/`, `lib/`)."""
    return _runtime.RUNTIME_DIR


def cmake_dir() -> str:
    """The directory of the runtime's CMake package (`lungoConfig.cmake`), for building
    generated packages."""
    return os.path.join(_runtime.RUNTIME_DIR, "lib", "cmake", "lungo")


__all__ = [
    "ABI_VERSION",
    "BOOL",
    "BYTE_ARRAY",
    "CHAR",
    "FLOAT",
    "FLOAT32",
    "FLOAT_ARRAY",
    "INT",
    "INT8",
    "INT16",
    "INT32",
    "INT64",
    "ISIZE",
    "NAT",
    "OPAQUE",
    "STRING",
    "UINT8",
    "UINT16",
    "UINT32",
    "UINT64",
    "UNIT",
    "USIZE",
    "Err",
    "HostError",
    "LeanError",
    "LeanFunction",
    "LeanIOError",
    "MalformedError",
    "Ok",
    "Opaque",
    "Program",
    "Some",
    "Type",
    "__version__",
    "array_of",
    "cmake_dir",
    "eio",
    "except_",
    "function",
    "inductive_expr",
    "io",
    "list_of",
    "option",
    "pair",
    "runtime_dir",
    "value",
]
