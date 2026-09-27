// Plain data passes between programs; a handle of one program is refused by another, since each
// runs in a WebAssembly instance of its own.
import assert from "node:assert/strict";
import { test } from "node:test";
import * as L from "lungo-ts";
import * as provider from "provider";
import * as consumer from "./consumer/index.js";

test("values pass between programs by value, not by handle", async () => {
  const prov = await provider.load();
  const cons = await consumer.load();
  assert.equal(cons.describe(prov.makePair(1n, "apples")), "apples: 1");
  const p = prov.mkPos(3n);
  assert.ok(p !== null);
  assert.throws(() => cons.double(p), L.MalformedError);
});
