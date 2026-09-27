/** An `IO.Error` of Lean: thrown by a Lean function, or by a host function to fail an `IO` extern. */
export class LeanIOError extends Error {
  constructor(message: string);
}

/** The error value of an `EIO ε` function; a host function of an `EIO ε` extern throws it. */
export class LeanError<E = unknown> extends Error {
  constructor(value: E);
  readonly value: E;
}

/** Arguments Lean cannot represent (a negative Nat, a number out of range, a closed value). */
export class MalformedError extends Error {}

export class Writer {
  constructor(program?: Program | null, result?: boolean);
  u8(v: number): void;
  u16(v: number): void;
  u32(v: number): void;
  u64(v: bigint): void;
  f64(v: number): void;
  f32(v: number): void;
  raw(bytes: Uint8Array): void;
  blob(bytes: Uint8Array): void;
  finish(): Uint8Array;
}

export class Reader {
  constructor(bytes: Uint8Array, program?: Program | null);
  u8(): number;
  u16(): number;
  u32(): number;
  u64(): bigint;
  f64(): number;
  f32(): number;
  count(unit?: number): number;
  blob(): Uint8Array;
  text(): string;
  finish(): void;
}

/** How JavaScript values of `T` cross the boundary as values of a Lean type. */
export class Type<T> {
  constructor(
    expr: Uint8Array,
    encode: (w: Writer, v: T) => void,
    decode: (r: Reader) => T,
    canBeNull?: boolean,
  );
  readonly expr: Uint8Array;
  readonly canBeNull: boolean;
  encode(w: Writer, v: T): void;
  decode(r: Reader): T;
}

export const MAX_FUNCTION_PARAMS: 15;

export const NAT: Type<bigint>;
export const INT: Type<bigint>;
export const BOOL: Type<boolean>;
export const UINT8: Type<number>;
export const UINT16: Type<number>;
export const UINT32: Type<number>;
export const UINT64: Type<bigint>;
export const USIZE: Type<bigint>;
export const INT8: Type<number>;
export const INT16: Type<number>;
export const INT32: Type<number>;
export const INT64: Type<bigint>;
export const ISIZE: Type<bigint>;
export const FLOAT: Type<number>;
export const FLOAT32: Type<number>;
/** `Char`: a string of one Unicode scalar value. */
export const CHAR: Type<string>;
export const STRING: Type<string>;
/** `Unit`, as `null`. */
export const UNIT: Type<null>;
export const BYTE_ARRAY: Type<Uint8Array>;
export const FLOAT_ARRAY: Type<number[]>;

/** A Lean value JavaScript does not represent, held by handle until closed or collected. */
export class Opaque {
  /** Releases the value. */
  close(): void;
}
export const OPAQUE: Type<Opaque>;

/** `some value` of an `Option` whose values can be `null` themselves. */
export class Some<T> {
  constructor(value: T);
  readonly value: T;
}

/** The JavaScript value of `Option α`: `null` or the value (`Some` when the value can be `null`). */
export type Option<T> = null | (null extends T ? Some<T> : T);

export function option<T>(t: Type<T>): Type<Option<T>>;
export function list<T>(t: Type<T>): Type<T[]>;
export function array<T>(t: Type<T>): Type<T[]>;
export function pair<A, B>(a: Type<A>, b: Type<B>): Type<[A, B]>;

/** `Except ε α`. */
export type Except<E, A> = { ok: true; value: A } | { ok: false; error: E };
export function except<E, A>(e: Type<E>, a: Type<A>): Type<Except<E, A>>;

export function inductiveExpr(index: number, ...args: Uint8Array[]): Uint8Array;

/** The type of Lean functions from `params` to `result`, as JavaScript functions. */
export function func<P extends unknown[], R>(
  params: { [K in keyof P]: Type<P[K]> },
  result: Type<R>,
): Type<(...args: P) => R>;

export interface Returns<T> {
  readonly kind: "value" | "io" | "eio";
}
export function value<T>(t: Type<T>): Returns<T>;
export function io<T>(t: Type<T>): Returns<T>;
export function eio<E, T>(e: Type<E>, t: Type<T>): Returns<T>;

/** A WASI implementation for a program module. */
export interface Wasi {
  readonly wasiImport: WebAssembly.ModuleImports;
  initialize(instance: WebAssembly.Instance): void;
}

/** A generated program: its WebAssembly module instance. */
export class Program {
  static load(module: WebAssembly.Module | BufferSource, options?: { wasi?: Wasi }): Promise<Program>;
  types(symbol: string): number;
  invoke<T>(symbol: string, typeArgs: Type<unknown>[], args: [Type<any>, unknown][], returns: Returns<T>): T;
  hostExtern(setSymbol: string, index: number, params: Type<any>[], returns: Returns<any>, f: (...args: any[]) => unknown): void;
  runMain(symbol: string, args: string[]): number;
}
