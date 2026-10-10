// The codec example from TypeScript (WebAssembly): the same proved codec, and its claims.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ASSURANCE, load } from "varint";

const p = await load();

// TEST0317: the proved codec from TypeScript
test("TEST0317 the proved codec from TypeScript", () => {
  assert.deepEqual(p.encode(300n), [0xac, 0x02]);
  assert.deepEqual(p.decode([0xe5, 0x8e, 0x26, 9]), [624485n, [9]]);
  assert.equal(p.decode([0x80, 0x81]), null, "input ending inside a number is refused");
  const ns = [0n, 127n, 128n, 2n ** 70n];
  assert.deepEqual(p.decodeAll(p.encodeAll(ns)), ns);
  const claim = ASSURANCE.claims.find((c) => c.name === "Varint.decode_encode");
  assert.deepEqual([claim.relation, claim.status], ["lungo.roundtrip", "proved"]);
});
