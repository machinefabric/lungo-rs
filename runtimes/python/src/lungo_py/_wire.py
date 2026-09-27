"""The wire format: little-endian fixed-width values and length-prefixed data."""

import struct

from ._errors import MalformedError

_U32 = 0xFFFFFFFF


class Writer:
    """Encodes values. `result`: the values are a result (their handles and host functions are
    given to the runtime) rather than arguments (lent for the call)."""

    __slots__ = ("program", "buf", "result", "temps")

    def __init__(self, program, result=False):
        self.program = program
        self.buf = bytearray()
        self.result = result
        # Host functions registered for the call, released when it returns.
        self.temps = []

    def u8(self, v):
        self.buf.append(v)

    def u16(self, v):
        self.buf += struct.pack("<H", v)

    def u32(self, v):
        self.buf += struct.pack("<I", v)

    def u64(self, v):
        self.buf += struct.pack("<Q", v)

    def f64(self, v):
        self.buf += struct.pack("<d", v)

    def f32(self, v):
        self.buf += struct.pack("<f", v)

    def length(self, n):
        if n > _U32:
            raise MalformedError(f"a length of {n} exceeds the wire format's limit")
        self.u32(n)

    def blob(self, b):
        self.length(len(b))
        self.buf += b


class Reader:
    """Decodes values the runtime produced."""

    __slots__ = ("program", "data", "pos")

    def __init__(self, program, data):
        self.program = program
        self.data = data
        self.pos = 0

    def take(self, n):
        if n < 0 or n > len(self.data) - self.pos:
            raise MalformedError(f"wire data ends after {len(self.data)} bytes; {n} more expected")
        b = self.data[self.pos : self.pos + n]
        self.pos += n
        return b

    def u8(self):
        return self.take(1)[0]

    def u16(self):
        return struct.unpack("<H", self.take(2))[0]

    def u32(self):
        return struct.unpack("<I", self.take(4))[0]

    def u64(self):
        return struct.unpack("<Q", self.take(8))[0]

    def f64(self):
        return struct.unpack("<d", self.take(8))[0]

    def f32(self):
        return struct.unpack("<f", self.take(4))[0]

    def count(self, unit=1):
        n = self.u32()
        if n * max(unit, 1) > len(self.data) - self.pos:
            raise MalformedError(f"a length of {n} exceeds the remaining wire data")
        return n

    def blob(self):
        return bytes(self.take(self.count(1)))

    def text(self):
        try:
            return self.blob().decode("utf-8")
        except UnicodeDecodeError:
            raise MalformedError("a string is not valid UTF-8") from None

    def finish(self):
        if self.pos != len(self.data):
            raise MalformedError(f"{len(self.data) - self.pos} unexpected bytes after the wire data")
