// The async-store example from TypeScript: the store is a Map the handler answers from,
// asynchronously.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ASSURANCE, load } from "store";

const p = await load();

/** A store in a Map, answering each operation after a turn of the event loop. */
const memory = (entries) => {
  const data = new Map(entries);
  const later = () => new Promise((r) => setTimeout(r, 0));
  return {
    data,
    async get(key) {
      await later();
      return data.has(key) ? data.get(key) : null;
    },
    async put(key, value) {
      await later();
      if (key === "readonly") throw new Error("the store is read-only there");
      data.set(key, value);
    },
  };
};

// TEST0338: the async store from TypeScript
test("TEST0338 the async store from TypeScript", async () => {
  const m = memory([["a", "1"], ["b", "2"]]);
  assert.equal(await p.copy(m, "a", "c"), true);
  assert.equal(await p.swap(m, "a", "b"), true);
  assert.equal(await p.swap(m, "a", "missing"), false);
  assert.deepEqual([...m.data].sort(), [["a", "2"], ["b", "1"], ["c", "1"]]);
  await assert.rejects(p.copy(m, "a", "readonly"), /read-only/);
  assert.equal(p.outstanding(), 0);
  const claim = ASSURANCE.claims.find((c) => c.name === "Store.swap_model");
  assert.deepEqual([claim.status, claim.specifications], ["proved", ["Store.Model"]]);
});
