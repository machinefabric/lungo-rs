// The shared wire-format vectors (compiler-tests/wire/vectors.json), which every language's
// support library must encode and decode exactly as the runtime does.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";
import * as L from "../index.js";

const vectors = JSON.parse(readFileSync(new URL("./vectors.json", import.meta.url), "utf8"));

const hex = (s) => Uint8Array.from(s.match(/../g) ?? [], (h) => parseInt(h, 16));
const toHex = (b) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");

// A type of JSON-represented values over a lungo-ts descriptor, so the vectors exercise it.
const dyn = (t, toJs, toJson) =>
  new L.Type(t.expr, (w, v) => t.encode(w, toJs(v)), (r) => toJson(t.decode(r)), t.canBeNull);
const id = (v) => v;
const bits = (width) => {
  const buf = new DataView(new ArrayBuffer(8));
  return [
    (v) => {
      const n = BigInt("0x" + v.bits);
      if (width === 8) {
        buf.setBigUint64(0, n);
        return buf.getFloat64(0);
      }
      buf.setUint32(0, Number(n));
      return buf.getFloat32(0);
    },
    (x) => {
      if (width === 8) {
        buf.setFloat64(0, x);
        return { bits: buf.getBigUint64(0).toString(16).padStart(16, "0") };
      }
      buf.setFloat32(0, x);
      return { bits: buf.getUint32(0).toString(16).padStart(8, "0") };
    },
  ];
};

function typeOf(r) {
  const tag = r.u8();
  switch (tag) {
    case 0: return dyn(L.NAT, BigInt, String);
    case 1: return dyn(L.INT, BigInt, String);
    case 2: return dyn(L.BOOL, id, id);
    case 3: return dyn(L.UINT8, id, id);
    case 4: return dyn(L.UINT16, id, id);
    case 5: return dyn(L.UINT32, id, id);
    case 6: return dyn(L.UINT64, BigInt, String);
    case 7: return dyn(L.USIZE, BigInt, String);
    case 8: return dyn(L.INT8, id, id);
    case 9: return dyn(L.INT16, id, id);
    case 10: return dyn(L.INT32, id, id);
    case 11: return dyn(L.INT64, BigInt, String);
    case 12: return dyn(L.ISIZE, BigInt, String);
    case 13: return dyn(L.FLOAT, ...bits(8));
    case 14: return dyn(L.FLOAT32, ...bits(4));
    case 15: return dyn(L.CHAR, id, id);
    case 16: return dyn(L.STRING, id, id);
    case 17: return dyn(L.UNIT, id, id);
    case 18: return dyn(L.BYTE_ARRAY, (v) => hex(v.bytes), (b) => ({ bytes: toHex(b) }));
    case 19: {
      const [f, g] = bits(8);
      return dyn(L.FLOAT_ARRAY, (v) => v.map((x) => f({ bits: x })), (xs) => xs.map((x) => g(x).bits));
    }
    case 20: {
      const inner = typeOf(r);
      return dyn(
        L.option(inner),
        (v) => ("none" in v ? null : inner.canBeNull ? new L.Some(v.some) : v.some),
        (x) => (x === null ? { none: null } : { some: x instanceof L.Some ? x.value : x }),
      );
    }
    case 21: return dyn(L.list(typeOf(r)), id, id);
    case 22: return dyn(L.array(typeOf(r)), id, id);
    case 23: {
      const a = typeOf(r);
      return dyn(L.pair(a, typeOf(r)), id, id);
    }
    case 24: {
      const e = typeOf(r);
      const a = typeOf(r);
      return dyn(
        L.except(e, a),
        (v) => ("ok" in v ? { ok: true, value: v.ok } : { ok: false, error: v.error }),
        (x) => (x.ok ? { ok: x.value } : { error: x.error }),
      );
    }
    case 25: {
      // Function values are handles and host callbacks naming live objects of a running
      // program: only their rejection is checked.
      const n = r.u32();
      const params = [];
      for (let i = 0; i < n; i++) params.push(typeOf(r));
      return L.func(params, typeOf(r));
    }
    case 28: return L.OPAQUE;
    default: throw new Error(`unknown type tag ${tag}`);
  }
}

const parse = (expr) => {
  const r = new L.Reader(hex(expr));
  const t = typeOf(r);
  r.finish();
  return t;
};

test("valid vectors round-trip", () => {
  let checked = 0;
  for (const v of vectors.valid) {
    if (v.type.startsWith("19") || v.type.startsWith("1c")) continue;
    const t = parse(v.type);
    const w = new L.Writer();
    t.encode(w, v.value);
    assert.equal(toHex(w.finish()), v.bytes, v.name);
    const r = new L.Reader(hex(v.bytes));
    assert.deepEqual(t.decode(r), v.value, v.name);
    r.finish();
    checked++;
  }
  assert.ok(checked > 40);
});

test("invalid vectors are rejected", () => {
  for (const v of vectors.invalid) {
    const t = parse(v.type);
    const r = new L.Reader(hex(v.bytes));
    assert.throws(
      () => {
        t.decode(r);
        r.finish();
      },
      L.MalformedError,
      v.name,
    );
  }
});

test("values Lean cannot represent are rejected", () => {
  const w = new L.Writer();
  for (const [t, v] of [
    [L.NAT, -1n],
    [L.NAT, 1],
    [L.UINT8, 256],
    [L.INT32, 1.5],
    [L.STRING, "\ud800"],
    [L.CHAR, "ab"],
    [L.option(L.option(L.NAT)), 5n],
    [L.pair(L.NAT, L.NAT), [1n]],
    [L.func([L.NAT], L.NAT), 3],
  ]) {
    assert.throws(() => t.encode(w, v), L.MalformedError, String(v));
  }
});
