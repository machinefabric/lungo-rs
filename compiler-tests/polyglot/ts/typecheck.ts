// The generated declarations type-check against their intended use.
import * as L from "lungo-ts";
import {
  ASSURANCE,
  load,
  type FetchOpHandler,
  type Lookup,
  type Point,
  type PolyglotFacilities,
  type Shape,
  type Tree,
} from "polyglot";

const facilities: PolyglotFacilities = {
  scaler: { hostScale: (n: bigint) => n },
  journal: { hostRecord: (_line: string) => {} },
};
const p = await load({ facilities });
const handler: FetchOpHandler = {
  get: async (url: string) => ({ ok: true, value: url }),
  stamp: (t) => t,
  scaler: (k: bigint) => (x: bigint) => x * k,
};
const fetched: Promise<Array<string>> = p.fetchAll(handler, ["a"], { signal: new AbortController().signal });
const waiting: number = p.outstanding();
const claims: readonly L.AssuranceClaim[] = ASSURANCE.claims;
const pt: Point = { x: 1, y: 2, label: "p", tag: 3 };
const s: Shape = { kind: "circle", center: pt, radius: 1 };
const area: number = p.area(s);
const hit: Lookup = p.lookup(3n, 1n);
const t: Tree<bigint> = p.ofList(L.NAT, [1n]);
const n: bigint = p.size(L.NAT, t);
const e: L.Except<string, bigint> = p.divide(1n, 2n);
const f: string | null = p.firstWord("x");
const code: number = p.runMain([]);
void [area, hit, n, e, f, code, fetched, waiting, claims];
