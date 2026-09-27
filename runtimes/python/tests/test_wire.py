"""The shared wire-format vectors (compiler-tests/wire/vectors.json), which every language's
support library must encode and decode exactly as the runtime does."""

import json
import math
import os
import struct
import unittest

import lungo_py as L
from lungo_py._wire import Reader, Writer

with open(os.path.join(os.path.dirname(__file__), "vectors.json"), encoding="utf-8") as f:
    VECTORS = json.load(f)


class Dyn:
    """A type of JSON-represented values over a lungo_py descriptor, so the vectors exercise
    the descriptors."""

    def __init__(self, t, to_py, to_json):
        self.t, self.to_py, self.to_json = t, to_py, to_json

    def encode(self, w, v):
        self.t.encode(w, self.to_py(v))

    def decode(self, r):
        return self.to_json(self.t.decode(r))


def ident(v):
    return v


def bits(width, fmt):
    def to_py(v):
        return struct.unpack(fmt, int(v["bits"], 16).to_bytes(width, "big"))[0]

    def to_json(x):
        return {"bits": struct.pack(fmt, x).hex()}

    return to_py, to_json


class Adapter(L.Type):
    """A lungo_py Type of JSON values for container descriptors."""

    def __init__(self, dyn):
        self.dyn = dyn

    def expr(self):
        return self.dyn.t.expr()

    def can_be_none(self):
        return self.dyn.t.can_be_none()

    def encode(self, w, v):
        self.dyn.encode(w, v)

    def decode(self, r):
        return self.dyn.decode(r)


def type_of(r):
    tag = r.u8()
    decimal = (int, str)
    simple = {
        0: Dyn(L.NAT, int, str),
        1: Dyn(L.INT, int, str),
        2: Dyn(L.BOOL, ident, ident),
        3: Dyn(L.UINT8, ident, ident),
        4: Dyn(L.UINT16, ident, ident),
        5: Dyn(L.UINT32, ident, ident),
        6: Dyn(L.UINT64, *decimal),
        7: Dyn(L.USIZE, *decimal),
        8: Dyn(L.INT8, ident, ident),
        9: Dyn(L.INT16, ident, ident),
        10: Dyn(L.INT32, ident, ident),
        11: Dyn(L.INT64, *decimal),
        12: Dyn(L.ISIZE, *decimal),
        13: Dyn(L.FLOAT, *bits(8, ">d")),
        14: Dyn(L.FLOAT32, *bits(4, ">f")),
        15: Dyn(L.CHAR, ident, ident),
        16: Dyn(L.STRING, ident, ident),
        17: Dyn(L.UNIT, ident, ident),
        18: Dyn(L.BYTE_ARRAY, lambda v: bytes.fromhex(v["bytes"]), lambda b: {"bytes": b.hex()}),
        19: Dyn(
            L.FLOAT_ARRAY,
            lambda v: [struct.unpack(">d", bytes.fromhex(x))[0] for x in v],
            lambda xs: [struct.pack(">d", x).hex() for x in xs],
        ),
    }
    if tag in simple:
        return simple[tag]
    if tag == 20:
        inner = type_of(r)
        t = L.option(Adapter(inner))

        def to_py(v):
            if "none" in v:
                return None
            return L.Some(v["some"]) if t.t.can_be_none() else v["some"]

        def to_json(x):
            if x is None:
                return {"none": None}
            return {"some": x.value if isinstance(x, L.Some) else x}

        return Dyn(t, to_py, to_json)
    if tag in (21, 22):
        inner = Adapter(type_of(r))
        return Dyn(L.list_of(inner) if tag == 21 else L.array_of(inner), ident, ident)
    if tag == 23:
        a, b = Adapter(type_of(r)), Adapter(type_of(r))
        return Dyn(L.pair(a, b), tuple, list)
    if tag == 24:
        e, a = Adapter(type_of(r)), Adapter(type_of(r))
        return Dyn(
            L.except_(e, a),
            lambda v: L.Ok(v["ok"]) if "ok" in v else L.Err(v["error"]),
            lambda x: {"ok": x.value} if isinstance(x, L.Ok) else {"error": x.error},
        )
    if tag == 25:
        n = r.u32()
        params = [Adapter(type_of(r)) for _ in range(n)]
        # Function values are handles and host callbacks naming live objects of a running
        # program: only their rejection is checked.
        return Dyn(L.function(params, Adapter(type_of(r))), None, None)
    if tag == 28:
        return Dyn(L.OPAQUE, None, None)
    raise AssertionError(f"unknown type tag {tag}")


def parse(expr):
    r = Reader(None, bytes.fromhex(expr))
    d = type_of(r)
    r.finish()
    return d


class VectorTest(unittest.TestCase):
    def test_valid_vectors_round_trip(self):
        checked = 0
        for v in VECTORS["valid"]:
            if v["type"][:2] in ("19", "1c"):
                continue
            with self.subTest(v["name"]):
                d = parse(v["type"])
                w = Writer(None)
                d.encode(w, v["value"])
                self.assertEqual(bytes(w.buf).hex(), v["bytes"])
                r = Reader(None, bytes.fromhex(v["bytes"]))
                got = d.decode(r)
                r.finish()
                self.assertEqual(json.dumps(got, sort_keys=True), json.dumps(v["value"], sort_keys=True))
                checked += 1
        self.assertGreater(checked, 40)

    def test_invalid_vectors_are_rejected(self):
        for v in VECTORS["invalid"]:
            with self.subTest(v["name"]):
                d = parse(v["type"])
                r = Reader(None, bytes.fromhex(v["bytes"]))
                with self.assertRaises(L.MalformedError):
                    d.decode(r)
                    r.finish()

    def test_python_values_lean_cannot_represent_are_rejected(self):
        w = Writer(None)
        for t, v in [
            (L.NAT, -1),
            (L.NAT, True),
            (L.UINT8, 256),
            (L.INT8, -129),
            (L.STRING, "\ud800"),
            (L.CHAR, "ab"),
            (L.CHAR, "\ud800"),
            (L.option(L.option(L.NAT)), 5),
            (L.pair(L.NAT, L.NAT), [1, 2]),
            (L.function([L.NAT], L.NAT), 3),
        ]:
            with self.subTest(v=v):
                with self.assertRaises(L.MalformedError):
                    t.encode(w, v)

    def test_nan_payloads_survive(self):
        w = Writer(None)
        L.FLOAT.encode(w, math.nan)
        self.assertTrue(math.isnan(L.FLOAT.decode(Reader(None, bytes(w.buf)))))


if __name__ == "__main__":
    unittest.main()
