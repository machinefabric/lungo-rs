// The generated declarations type-check against their intended use.
import * as L from "lungo-ts";
import { load, type Point, type Shape, type Tree, type PolyglotHost } from "polyglot";

const host: PolyglotHost = { hostScale: (n: bigint) => n, hostRecord: (_line: string) => {} };
const p = await load({ host });
const pt: Point = { x: 1, y: 2, label: "p", tag: 3 };
const s: Shape = { kind: "circle", center: pt, radius: 1 };
const area: number = p.area(s);
const t: Tree<bigint> = p.ofList(L.NAT, [1n]);
const n: bigint = p.size(L.NAT, t);
const e: L.Except<string, bigint> = p.divide(1n, 2n);
const f: string | null = p.firstWord("x");
const code: number = p.runMain([]);
void [area, n, e, f, code];
