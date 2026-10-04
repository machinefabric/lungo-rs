// The polyglot assertions, in JavaScript, on the generated TypeScript package (WebAssembly).
import assert from "node:assert/strict";
import { test } from "node:test";
import * as L from "lungo-ts";
import { load } from "polyglot";

const lines = [];
const p = await load({
  host: {
    hostScale: (n) => n * 10n,
    hostRecord: (line) => {
      if (line === "fail") throw new L.LeanIOError("the host refuses to record `fail`");
      lines.push(line);
    },
  },
});

// TEST0048: numbers
test("TEST0048 numbers", () => {
  assert.equal(p.factorial(25n), 15511210043330985984000000n);
  assert.equal(p.negate(-123456789012345678901234567890n), 123456789012345678901234567890n);
  assert.throws(() => p.factorial(-1n), L.MalformedError);
  assert.throws(() => p.factorial(3), L.MalformedError);
});

// TEST0049: structures and inductives
test("TEST0049 structures and inductives", () => {
  const pt = { x: 1.5, y: -2, label: "p", tag: 7 };
  const moved = p.moveBy(pt, 1, 0.5);
  assert.deepEqual(moved, { x: 2.5, y: -1.5, label: "p", tag: 7 });
  assert.equal(p.describe(moved), "p#7 at (2.500000, -1.500000)");
  assert.equal(p.area({ kind: "rect", corner: moved, width: 3, height: 4 }), 12);
  assert.equal(p.area({ kind: "circle", center: pt, radius: 2 }), 12);
  assert.equal(p.area({ kind: "empty" }), 0);
  assert.throws(() => p.area({ kind: "triangle" }), L.MalformedError);
  assert.throws(() => p.moveBy({ ...pt, tag: 256 }, 0, 0), L.MalformedError);
});

// TEST0050: `Lookup` the type and `Registry.lookup` the function: `Lookup` and `lookup`.
test("TEST0050 a function named like its type", () => {
  assert.deepEqual(p.lookup(3n, 1n), { kind: "found", slot: 1n });
  assert.deepEqual(p.lookup(3n, 5n), { kind: "missing" });
});

// TEST0051: scalars and containers
test("TEST0051 scalars and containers", () => {
  assert.equal(p.mix(2n ** 64n - 1n, -5, "λ", true), "18446744073709551615/-5/λ/true");
  assert.deepEqual(p.reverseBytes(Uint8Array.of(1, 2, 3)), Uint8Array.of(3, 2, 1));
  assert.equal(p.sumFloats([0.5, 0.25, 2]), 2.75);
  assert.equal(p.firstWord("hello world"), "hello");
  assert.equal(p.firstWord(""), null);
  assert.deepEqual(p.swap(["x", 9n]), [9n, "x"]);
  assert.deepEqual(p.evens(10n), [0n, 2n, 4n, 6n, 8n]);
  assert.deepEqual(p.divide(10n, 0n), { ok: false, error: "division by zero" });
  assert.deepEqual(p.divide(10n, 3n), { ok: true, value: 3n });
});

// TEST0052: errors
test("TEST0052 errors", () => {
  assert.throws(() => p.checkedDiv(10n, 0n), (e) => e instanceof L.LeanIOError && e.message === "checkedDiv: division by zero");
  assert.throws(() => p.parseDigit("x"), (e) => e instanceof L.LeanError && e.value === "not a digit: x");
  assert.equal(p.parseDigit("7"), 7n);
});

// TEST0053: polymorphism
test("TEST0053 polymorphism", () => {
  const tree = p.ofList(L.NAT, [1n, 2n, 3n]);
  assert.equal(p.size(L.NAT, tree), 3n);
  const mirrored = p.mirror(L.NAT, tree);
  assert.equal(mirrored.kind, "node");
  assert.deepEqual(p.toList(L.NAT, mirrored), [3n, 2n, 1n]);
  const strings = { kind: "node", left: { kind: "leaf" }, value: "s", right: { kind: "leaf" } };
  assert.equal(p.size(L.STRING, strings), 1n);
  assert.throws(() => p.size(L.NAT, strings), L.MalformedError);
});

// TEST0054: functions
test("TEST0054 functions", () => {
  const calls = [];
  assert.equal(p.applyTwice((x) => (calls.push(x), 2n * x + 1n), 3n), 15n);
  assert.deepEqual(calls, [3n, 7n]);
  assert.equal(p.adder(5n, 3n), 8n);
});

// TEST0055: opaque values
test("TEST0055 opaque values", () => {
  const counter = p.newCounter(10n);
  assert.equal(p.bump(counter), 11n);
  assert.equal(p.bump(counter), 12n);
  counter.close();
  assert.throws(() => p.bump(counter), L.MalformedError);
});

// TEST0056: host externs
test("TEST0056 host externs", () => {
  assert.equal(p.scaledSum([1n, 2n, 3n]), 60n);
  lines.length = 0;
  assert.equal(p.recordAll(["a", "b"]), 2n);
  assert.throws(() => p.recordAll(["c", "fail", "d"]), (e) => e instanceof L.LeanIOError && e.message.includes("refuses to record"));
  assert.deepEqual(lines, ["a", "b", "c"]);
});

// TEST0058: main
test("TEST0058 main", () => {
  assert.equal(p.runMain(["one", "two"]), 2);
});

// TEST0062: the browser WASI runs the program too
test("TEST0062 the browser WASI runs the program too", async () => {
  const { BrowserWasi } = await import("lungo-ts/wasi.js");
  const out = [];
  const q = await load({
    wasi: new BrowserWasi({ stdout: (b) => out.push(new TextDecoder().decode(b)) }),
    host: { hostScale: (n) => n, hostRecord: () => {} },
  });
  assert.equal(q.factorial(20n), 2432902008176640000n);
  assert.equal(q.runMain([]), 0);
  assert.match(out.join(""), /polyglot \[\]/);
});
