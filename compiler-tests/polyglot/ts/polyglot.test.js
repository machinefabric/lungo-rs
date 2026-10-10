// The polyglot assertions, in JavaScript, on the generated TypeScript package (WebAssembly).
import assert from "node:assert/strict";
import { test } from "node:test";
import * as L from "lungo-ts";
import { spawnSync } from "node:child_process";
import { ASSURANCE, load } from "polyglot";

const lines = [];
const p = await load({
  facilities: {
    scaler: { hostScale: (n) => n * 10n },
    journal: {
      hostRecord: (line) => {
        if (line === "fail") throw new L.LeanIOError("the host refuses to record `fail`");
        lines.push(line);
      },
    },
  },
});

/** Answers the program's operations: `get` waits for `release` when the url is "slow", and
 * rejects for "broken". */
const fetcher = (release) => ({
  async get(url) {
    if (url === "slow") await release;
    if (url === "broken") throw new Error("the network is down");
    if (url === "missing") return { ok: false, error: "not found" };
    return { ok: true, value: `body:${url}` };
  },
  stamp: (t) => t,
  scaler: (k) => (x) => x * k,
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

// TEST0056: host facilities
test("TEST0056 host facilities", () => {
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
    facilities: { scaler: { hostScale: (n) => n }, journal: { hostRecord: () => {} } },
  });
  assert.equal(q.factorial(20n), 2432902008176640000n);
  assert.equal(q.runMain([]), 0);
  assert.match(out.join(""), /polyglot \[\]/);
});

// TEST0297: loading without every facility fails, naming the facility and an operation of it
test("TEST0297 a missing facility is named", async () => {
  await assert.rejects(
    load({ facilities: { journal: { hostRecord: () => {} } } }),
    (e) => e instanceof L.MissingFacilityError && e.facility === "polyglot.scaler" && e.operation === "hostScale",
  );
  await assert.rejects(
    load({ facilities: { journal: { hostRecord: () => {} }, scaler: {} } }),
    (e) => e instanceof L.MissingFacilityError && e.facility === "polyglot.scaler",
  );
});

// TEST0298: the package's assurance document is the program's: its claims, what they assume, and
// the facilities each export needs
test("TEST0298 assurance is the program's assurance", () => {
  const a = ASSURANCE;
  assert.deepEqual([a.program, a.schema_version, a.provenance.lean_version], ["polyglot", 1, "4.34.1"]);
  const claim = (name) => a.claims.find((c) => c.name === name);
  const exp = (name) => a.exports.find((e) => e.name === name);
  const c = claim("Polyglot.factorial_pos");
  assert.deepEqual([c.status, c.relation, c.assumptions], ["proved", "lungo.law", []]);
  assert.equal(claim("Polyglot.Tree.mirror_mirror").relation, "lungo.roundtrip");
  assert.deepEqual(claim("Polyglot.divide_ok").specifications, ["Polyglot.natDiv"]);
  // Proved, and conditional on what the host's scaler is assumed to do.
  const mono = claim("Polyglot.scaledSum_singleton_mono");
  assert.deepEqual([mono.status, mono.assumptions], ["proved", ["Polyglot.ScalesMonotonically"]]);
  assert.deepEqual(exp("Polyglot.scaledSum").facilities, ["Polyglot.Scaler"]);
  assert.deepEqual(exp("Polyglot.scaledSum").assumptions, ["Polyglot.ScalesMonotonically"]);
  assert.deepEqual([exp("Polyglot.fetchAll").async, exp("Polyglot.fetchAll").facilities], [true, ["Polyglot.fetchInterface"]]);
  // Sorted by Lean name.
  assert.deepEqual(
    a.facilities.map((c) => `${c.id}/${c.form}`),
    ["polyglot.journal/extern", "polyglot.scaler/extern", "polyglot.fetch/async"],
  );
  assert.deepEqual(exp("Polyglot.negate").claims, []);
  assert.ok(Object.isFrozen(a) && Object.isFrozen(a.claims[0]), "the document is frozen");
});

// TEST0299: an async program runs on its handler's answers: data, an opaque handle passed back,
// and a host function the program calls
test("TEST0299 an async program runs on the handler's answers", async () => {
  const h = fetcher();
  assert.deepEqual(await p.fetchAll(h, ["a", "missing", "b"]), ["body:a", "error: not found", "body:b"]);
  const token = p.mkToken(5n);
  assert.ok(token !== null);
  assert.equal(await p.stampToken(h, token), 6n);
  assert.equal(await p.applyScaler(h, 7n, 6n), 42n);
  assert.equal(p.outstanding(), 0);
});

// TEST0300: a rejection of the handler abandons the program: the call rejects with it, and nothing
// waits
test("TEST0300 a handler's error abandons the program", async () => {
  await assert.rejects(p.fetchAll(fetcher(), ["a", "broken", "b"]), /the network is down/);
  assert.equal(p.outstanding(), 0);
});

// TEST0301: aborting the signal abandons a program waiting for an answer
test("TEST0301 cancellation abandons the program", async () => {
  const controller = new AbortController();
  const call = p.fetchAll(fetcher(new Promise(() => {})), ["slow"], { signal: controller.signal });
  await new Promise((r) => setTimeout(r, 20));
  assert.equal(p.outstanding(), 1);
  controller.abort();
  await assert.rejects(call, (e) => e.name === "AbortError");
  assert.equal(p.outstanding(), 0);
});

// TEST0302: many async programs waiting at once resume independently, answered in any order
test("TEST0302 concurrent async programs resume independently", async () => {
  let open;
  const release = new Promise((r) => (open = r));
  const h = fetcher(release);
  const calls = Array.from({ length: 100 }, async (_, i) => {
    await new Promise((r) => setTimeout(r, Math.random() * 20));
    return p.fetchAll(h, ["slow", "x".repeat(i)]);
  });
  await new Promise((r) => setTimeout(r, 50));
  assert.equal(p.outstanding(), 100);
  open();
  (await Promise.all(calls)).forEach((got, i) => assert.deepEqual(got, ["body:slow", `body:${"x".repeat(i)}`]));
  assert.equal(p.outstanding(), 0);
});

// TEST0303: a process may end while a program waits for an answer
test("TEST0303 a process ends with a program waiting", () => {
  const script = `
    import { load } from "polyglot";
    const p = await load({ facilities: { scaler: { hostScale: (n) => n }, journal: { hostRecord: () => {} } } });
    p.fetchAll({ get: () => new Promise(() => {}) }, ["slow"]);
    await new Promise((r) => setTimeout(r, 10));
    console.log("waiting:", p.outstanding());
    process.exit(0);
  `;
  const out = spawnSync(process.execPath, ["--input-type=module", "-e", script], { encoding: "utf8" });
  assert.deepEqual([out.status, out.stdout], [0, "waiting: 1\n"], out.stderr);
});
