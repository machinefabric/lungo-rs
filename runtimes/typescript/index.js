// lungo-ts: the support library of the TypeScript/JavaScript packages lungo generates from Lean
// programs. A generated package loads its program's WebAssembly module through `Program.load`
// and calls it with values described by `Type`s: `bigint` for Nat, Int and 64-bit integers,
// `number` for smaller integers and floats, plain objects for structures, tagged objects
// (`kind`) for inductive types.

// ---------------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------------

/** An `IO.Error` of Lean: thrown by a Lean function, or by a host function to fail an `IO` extern
 * (`new LeanIOError(message)` is `IO.userError`). */
export class LeanIOError extends Error {
  constructor(message, handle = null) {
    super(message);
    this.name = "LeanIOError";
    this.handle = handle;
  }
}

/** The error value of an `EIO ε` function; a host function of an `EIO ε` extern throws it. */
export class LeanError extends Error {
  constructor(value) {
    super(`Lean error: ${typeof value === "string" ? value : JSON.stringify(value, bigintJson)}`);
    this.name = "LeanError";
    this.value = value;
  }
}

/** Arguments Lean cannot represent (a negative Nat, a number out of range, a closed value). */
export class MalformedError extends Error {
  constructor(message) {
    super(message);
    this.name = "MalformedError";
  }
}

function bigintJson(_key, v) {
  return typeof v === "bigint" ? v.toString() : v;
}

function malformed(message) {
  return new MalformedError(message);
}

// ---------------------------------------------------------------------------------------------
// The wire format
// ---------------------------------------------------------------------------------------------

const utf8 = new TextEncoder();
const fromUtf8 = new TextDecoder("utf-8", { fatal: true });

/** Encodes values. `result`: the values are a result (handles and host functions are given to the
 * runtime) rather than arguments (lent for the call). */
export class Writer {
  constructor(program = null, result = false) {
    this.program = program;
    this.result = result;
    this.temps = [];
    this.bytes = new Uint8Array(64);
    this.length = 0;
  }

  reserve(n) {
    if (this.length + n > this.bytes.length) {
      const next = new Uint8Array(Math.max(this.bytes.length * 2, this.length + n));
      next.set(this.bytes.subarray(0, this.length));
      this.bytes = next;
    }
  }

  view(n) {
    this.reserve(n);
    const v = new DataView(this.bytes.buffer, this.length, n);
    this.length += n;
    return v;
  }

  u8(v) {
    this.view(1).setUint8(0, v);
  }
  u16(v) {
    this.view(2).setUint16(0, v, true);
  }
  u32(v) {
    this.view(4).setUint32(0, v, true);
  }
  u64(v) {
    this.view(8).setBigUint64(0, v, true);
  }
  f64(v) {
    this.view(8).setFloat64(0, v, true);
  }
  f32(v) {
    this.view(4).setFloat32(0, v, true);
  }

  raw(bytes) {
    this.reserve(bytes.length);
    this.bytes.set(bytes, this.length);
    this.length += bytes.length;
  }

  lengthPrefix(n) {
    if (n > 0xffffffff) throw malformed(`a length of ${n} exceeds the wire format's limit`);
    this.u32(n);
  }

  blob(bytes) {
    this.lengthPrefix(bytes.length);
    this.raw(bytes);
  }

  finish() {
    return this.bytes.slice(0, this.length);
  }
}

/** Decodes values the runtime produced. */
export class Reader {
  constructor(bytes, program = null) {
    this.bytes = bytes;
    this.pos = 0;
    this.program = program;
    this.dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  }

  take(n) {
    if (n < 0 || n > this.bytes.length - this.pos) {
      throw malformed(`wire data ends after ${this.bytes.length} bytes; ${n} more expected`);
    }
    const at = this.pos;
    this.pos += n;
    return at;
  }

  u8() {
    return this.dv.getUint8(this.take(1));
  }
  u16() {
    return this.dv.getUint16(this.take(2), true);
  }
  u32() {
    return this.dv.getUint32(this.take(4), true);
  }
  u64() {
    return this.dv.getBigUint64(this.take(8), true);
  }
  f64() {
    return this.dv.getFloat64(this.take(8), true);
  }
  f32() {
    return this.dv.getFloat32(this.take(4), true);
  }

  count(unit = 1) {
    const n = this.u32();
    if (n * Math.max(unit, 1) > this.bytes.length - this.pos) {
      throw malformed(`a length of ${n} exceeds the remaining wire data`);
    }
    return n;
  }

  blob() {
    const n = this.count(1);
    const at = this.take(n);
    return this.bytes.slice(at, at + n);
  }

  text() {
    try {
      return fromUtf8.decode(this.blob());
    } catch {
      throw malformed("a string is not valid UTF-8");
    }
  }

  finish() {
    if (this.pos !== this.bytes.length) {
      throw malformed(`${this.bytes.length - this.pos} unexpected bytes after the wire data`);
    }
  }
}

// ---------------------------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------------------------

const TAG = {
  nat: 0, int: 1, bool: 2, uint8: 3, uint16: 4, uint32: 5, uint64: 6, usize: 7, int8: 8, int16: 9,
  int32: 10, int64: 11, isize: 12, float: 13, float32: 14, char: 15, string: 16, unit: 17,
  byteArray: 18, floatArray: 19, option: 20, list: 21, array: 22, prod: 23, except: 24,
  function: 25, inductive: 27, opaque: 28,
};

export const MAX_FUNCTION_PARAMS = 15;

/** How JavaScript values of `T` cross the boundary as values of a Lean type. */
export class Type {
  constructor(expr, encode, decode, canBeNull = false) {
    this.expr = expr;
    this.encode = encode;
    this.decode = decode;
    this.canBeNull = canBeNull;
  }
}

function simple(tag, encode, decode, canBeNull = false) {
  return new Type(Uint8Array.of(tag), encode, decode, canBeNull);
}

function putMagnitude(w, m) {
  const bytes = [];
  while (m > 0n) {
    bytes.push(Number(m & 0xffn));
    m >>= 8n;
  }
  w.blob(Uint8Array.from(bytes));
}

function magnitude(r) {
  const b = r.blob();
  if (b.length > 0 && b[b.length - 1] === 0) throw malformed("a number's magnitude has a leading zero byte");
  let m = 0n;
  for (let i = b.length - 1; i >= 0; i--) m = (m << 8n) | BigInt(b[i]);
  return m;
}

function checkBigint(v, lo, hi, name) {
  if (typeof v !== "bigint" || (lo !== null && v < lo) || (hi !== null && v > hi)) {
    throw malformed(`${String(v)} is not a ${name} (bigint)`);
  }
  return v;
}

function checkNumber(v, lo, hi, name) {
  if (typeof v !== "number" || !Number.isInteger(v) || v < lo || v > hi) {
    throw malformed(`${String(v)} is not a ${name}`);
  }
  return v;
}

export const NAT = simple(
  TAG.nat,
  (w, v) => putMagnitude(w, checkBigint(v, 0n, null, "Nat")),
  magnitude,
);

export const INT = simple(
  TAG.int,
  (w, v) => {
    checkBigint(v, null, null, "Int");
    w.u8(v < 0n ? 1 : 0);
    putMagnitude(w, v < 0n ? -v : v);
  },
  (r) => {
    const sign = r.u8();
    const m = magnitude(r);
    if (sign === 0) return m;
    if (sign === 1 && m === 0n) throw malformed("negative zero is not a canonical Int");
    if (sign === 1) return -m;
    throw malformed(`invalid Int sign ${sign}`);
  },
);

export const BOOL = simple(
  TAG.bool,
  (w, v) => {
    if (typeof v !== "boolean") throw malformed(`${String(v)} is not a Bool`);
    w.u8(v ? 1 : 0);
  },
  (r) => {
    const b = r.u8();
    if (b > 1) throw malformed(`invalid Bool ${b}`);
    return b === 1;
  },
);

export const UINT8 = simple(TAG.uint8, (w, v) => w.u8(checkNumber(v, 0, 0xff, "UInt8")), (r) => r.u8());
export const UINT16 = simple(TAG.uint16, (w, v) => w.u16(checkNumber(v, 0, 0xffff, "UInt16")), (r) => r.u16());
export const UINT32 = simple(TAG.uint32, (w, v) => w.u32(checkNumber(v, 0, 0xffffffff, "UInt32")), (r) => r.u32());
export const UINT64 = simple(TAG.uint64, (w, v) => w.u64(checkBigint(v, 0n, 0xffffffffffffffffn, "UInt64")), (r) => r.u64());
/** `USize`, as a bigint (the runtime rejects values its platform's USize cannot hold). */
export const USIZE = simple(TAG.usize, (w, v) => w.u64(checkBigint(v, 0n, 0xffffffffffffffffn, "USize")), (r) => r.u64());
export const INT8 = simple(TAG.int8, (w, v) => w.u8(checkNumber(v, -0x80, 0x7f, "Int8") & 0xff), (r) => (r.u8() << 24) >> 24);
export const INT16 = simple(TAG.int16, (w, v) => w.u16(checkNumber(v, -0x8000, 0x7fff, "Int16") & 0xffff), (r) => (r.u16() << 16) >> 16);
export const INT32 = simple(TAG.int32, (w, v) => w.u32(checkNumber(v, -0x80000000, 0x7fffffff, "Int32") >>> 0), (r) => r.u32() | 0);
const I64 = [-(1n << 63n), (1n << 63n) - 1n];
export const INT64 = simple(TAG.int64, (w, v) => w.u64(BigInt.asUintN(64, checkBigint(v, ...I64, "Int64"))), (r) => BigInt.asIntN(64, r.u64()));
/** `ISize`, as a bigint. */
export const ISIZE = simple(TAG.isize, (w, v) => w.u64(BigInt.asUintN(64, checkBigint(v, ...I64, "ISize"))), (r) => BigInt.asIntN(64, r.u64()));

function checkFloat(v) {
  if (typeof v !== "number") throw malformed(`${String(v)} is not a Float`);
  return v;
}

export const FLOAT = simple(TAG.float, (w, v) => w.f64(checkFloat(v)), (r) => r.f64());
export const FLOAT32 = simple(TAG.float32, (w, v) => w.f32(checkFloat(v)), (r) => r.f32());

function scalarValue(c) {
  return c >= 0 && c <= 0x10ffff && !(c >= 0xd800 && c <= 0xdfff);
}

/** `Char`: a string of one Unicode scalar value. */
export const CHAR = simple(
  TAG.char,
  (w, v) => {
    const cp = typeof v === "string" ? v.codePointAt(0) : undefined;
    if (cp === undefined || String.fromCodePoint(cp) !== v || !scalarValue(cp)) throw malformed(`${String(v)} is not a Char`);
    w.u32(cp);
  },
  (r) => {
    const c = r.u32();
    if (!scalarValue(c)) throw malformed(`${c} is not a Unicode scalar value`);
    return String.fromCodePoint(c);
  },
);

export const STRING = simple(
  TAG.string,
  (w, v) => {
    if (typeof v !== "string" || !v.isWellFormed()) throw malformed(`${String(v)} is not a String (well-formed Unicode)`);
    w.blob(utf8.encode(v));
  },
  (r) => r.text(),
);

/** `Unit`, as `null`. */
export const UNIT = simple(
  TAG.unit,
  (_w, v) => {
    if (v !== null) throw malformed(`${String(v)} is not Unit (null)`);
  },
  () => null,
  true,
);

export const BYTE_ARRAY = simple(
  TAG.byteArray,
  (w, v) => {
    if (!(v instanceof Uint8Array)) throw malformed("a ByteArray is a Uint8Array");
    w.blob(v);
  },
  (r) => r.blob(),
);

export const FLOAT_ARRAY = simple(
  TAG.floatArray,
  (w, v) => {
    if (!Array.isArray(v) && !(v instanceof Float64Array)) throw malformed("a FloatArray is an array of numbers");
    w.lengthPrefix(v.length);
    for (const x of v) w.f64(checkFloat(x));
  },
  (r) => {
    const n = r.count(8);
    const out = [];
    for (let i = 0; i < n; i++) out.push(r.f64());
    return out;
  },
);

const handleRegistry = new FinalizationRegistry(({ program, id }) => program.releaseHandle(id));

/** A Lean value JavaScript does not represent, held by handle until closed or collected. */
export class Opaque {
  constructor(program, id) {
    this.program = program;
    this.id = id;
    handleRegistry.register(this, { program, id }, this);
  }

  live() {
    if (this.id === null) throw malformed("the Lean value was closed");
    return this.id;
  }

  /** Releases the value. */
  close() {
    if (this.id !== null) {
      handleRegistry.unregister(this);
      this.program.releaseHandle(this.id);
      this.id = null;
    }
  }
}

function writeHandle(w, value) {
  const id = value.live();
  w.u64(w.result ? w.program.cloneHandle(id) : id);
}

export const OPAQUE = simple(
  TAG.opaque,
  (w, v) => {
    if (!(v instanceof Opaque)) throw malformed("not an opaque Lean value");
    writeHandle(w, v);
  },
  (r) => new Opaque(r.program, r.u64()),
);

/** `some value` of an `Option` whose values can be `null` themselves (`Option (Option α)`,
 * `Option Unit`); other options are `value` or `null`. */
export class Some {
  constructor(value) {
    this.value = value;
  }
}

/** `Option α`: `null` or the value (`new Some(value)` when the value can be `null`). */
export function option(t) {
  return new Type(
    Uint8Array.of(TAG.option, ...t.expr),
    (w, v) => {
      if (v === null) return w.u8(0);
      if (t.canBeNull) {
        if (!(v instanceof Some)) throw malformed("not null or a Some");
        v = v.value;
      }
      w.u8(1);
      t.encode(w, v);
    },
    (r) => {
      const tag = r.u8();
      if (tag === 0) return null;
      if (tag === 1) {
        const v = t.decode(r);
        return t.canBeNull ? new Some(v) : v;
      }
      throw malformed(`invalid Option tag ${tag}`);
    },
    true,
  );
}

function sequence(tag, t) {
  return new Type(
    Uint8Array.of(tag, ...t.expr),
    (w, v) => {
      if (!Array.isArray(v)) throw malformed("not an array");
      w.lengthPrefix(v.length);
      for (const x of v) t.encode(w, x);
    },
    (r) => {
      const n = r.count(1);
      const out = [];
      for (let i = 0; i < n; i++) out.push(t.decode(r));
      return out;
    },
  );
}

/** `List α`, as an array. */
export const list = (t) => sequence(TAG.list, t);
/** `Array α`. */
export const array = (t) => sequence(TAG.array, t);

/** `α × β`, as `[a, b]`. */
export function pair(a, b) {
  return new Type(
    Uint8Array.of(TAG.prod, ...a.expr, ...b.expr),
    (w, v) => {
      if (!Array.isArray(v) || v.length !== 2) throw malformed("not a pair");
      a.encode(w, v[0]);
      b.encode(w, v[1]);
    },
    (r) => {
      const x = a.decode(r);
      return [x, b.decode(r)];
    },
  );
}

/** `Except ε α`, as `{ ok: true, value }` or `{ ok: false, error }`. */
export function except(e, a) {
  return new Type(
    Uint8Array.of(TAG.except, ...e.expr, ...a.expr),
    (w, v) => {
      if (v?.ok === true) {
        w.u8(1);
        a.encode(w, v.value);
      } else if (v?.ok === false) {
        w.u8(0);
        e.encode(w, v.error);
      } else {
        throw malformed("not { ok, value } or { ok, error }");
      }
    },
    (r) => {
      const tag = r.u8();
      if (tag === 0) return { ok: false, error: e.decode(r) };
      if (tag === 1) return { ok: true, value: a.decode(r) };
      throw malformed(`invalid Except tag ${tag}`);
    },
  );
}

function le32(n) {
  return [n & 0xff, (n >>> 8) & 0xff, (n >>> 16) & 0xff, (n >>> 24) & 0xff];
}

/** The type expression of type `index` of a program's type table applied to `args`. */
export function inductiveExpr(index, ...args) {
  return Uint8Array.from([TAG.inductive, ...le32(index), ...le32(args.length), ...args.flatMap((a) => [...a])]);
}

const leanFunctions = new WeakMap();

/** The type of Lean functions from `params` to `result`, as JavaScript functions. A JavaScript
 * function passed to Lean may be called while the program runs; an exception it throws is fatal
 * to the Lean program, since Lean's functions cannot fail. */
export function func(params, result) {
  if (params.length < 1 || params.length > MAX_FUNCTION_PARAMS) {
    throw new RangeError(`a function type of ${params.length} parameters`);
  }
  const expr = Uint8Array.from([TAG.function, ...le32(params.length), ...params.flatMap((p) => [...p.expr]), ...result.expr]);
  const type = new Type(
    expr,
    (w, f) => {
      if (typeof f !== "function") throw malformed("not a function");
      const lean = leanFunctions.get(f);
      if (lean) {
        w.u8(0);
        writeHandle(w, lean);
        return;
      }
      w.u8(1);
      w.u64(w.program.hostFunction(w, (r, out) => {
        const args = params.map((p) => p.decode(r));
        let v;
        try {
          v = f(...args);
        } catch (e) {
          throw new Error(`a JavaScript function passed to Lean failed: ${e}`);
        }
        result.encode(out, v);
      }));
    },
    (r) => {
      const kind = r.u8();
      const id = r.u64();
      if (kind !== 0) throw malformed(`the runtime sent a function of kind ${kind}`);
      const handle = new Opaque(r.program, id);
      const program = r.program;
      const f = (...args) => program.callClosure(handle, type, params, result, args);
      leanFunctions.set(f, handle);
      return f;
    },
  );
  return type;
}

// ---------------------------------------------------------------------------------------------
// Programs
// ---------------------------------------------------------------------------------------------

/** What a function returns: a value, an `IO` result, or an `EIO ε` result. */
export const value = (t) => ({ kind: "value", value: t });
export const io = (t) => ({ kind: "io", value: t });
export const eio = (e, t) => ({ kind: "eio", error: e, value: t });

async function nodeWasi() {
  const { WASI } = await import("node:wasi");
  return new WASI({ version: "preview1", args: [], env: {}, returnOnExit: true });
}

/** A generated program: its WebAssembly module instance. */
export class Program {
  /** Loads the program module `module` (a `WebAssembly.Module`, or its bytes). `wasi` is the
   * WASI implementation (default: Node's `node:wasi`, else the minimal `BrowserWasi`). */
  static async load(module, { wasi } = {}) {
    if (!wasi) {
      if (typeof process !== "undefined" && process.versions?.node) {
        wasi = await nodeWasi();
      } else {
        const { BrowserWasi } = await import("./wasi.js");
        wasi = new BrowserWasi();
      }
    }
    const program = new Program();
    const imports = {
      wasi_snapshot_preview1: wasi.wasiImport,
      lungo: {
        dispatch: (callback, input, len, out) => program.dispatch(callback, input, len, out),
        retain: (callback) => program.retain(callback),
        release: (callback) => program.release(callback),
      },
    };
    const compiled = module instanceof WebAssembly.Module ? module : await WebAssembly.compile(module);
    const instance = await WebAssembly.instantiate(compiled, imports);
    wasi.initialize(instance);
    program.exports = instance.exports;
    program.exports.lungo_wasm_use_host();
    return program;
  }

  constructor() {
    this.exports = null;
    this.hosts = new Map();
    this.nextHost = 0n;
    this.typesPtr = null;
  }

  memory() {
    return new Uint8Array(this.exports.memory.buffer);
  }

  /** The program's type table (`<prefix>types`). */
  types(symbol) {
    if (this.typesPtr === null) this.typesPtr = this.exports[symbol]();
    return this.typesPtr;
  }

  /** Copies `bytes` into the module's memory; the address, to free with `free`. */
  copyIn(bytes) {
    const ptr = this.exports.lungo_wasm_alloc(bytes.length);
    this.memory().set(bytes, ptr);
    return ptr;
  }

  free(ptr, len) {
    this.exports.lungo_wasm_free(ptr, len);
  }

  /** Runs `call(inputPtr, inputLen, bufferPtr)` on `input` and returns its status and output. */
  withBuffers(input, call) {
    const ptr = this.copyIn(input);
    const buf = this.exports.lungo_wasm_buffer_new();
    try {
      const status = call(ptr, input.length, buf);
      const dv = new DataView(this.exports.memory.buffer);
      const data = dv.getUint32(buf, true);
      const len = dv.getUint32(buf + 4, true);
      const out = this.memory().slice(data, data + len);
      this.exports.lungo_buffer_free(buf);
      return { status, out };
    } finally {
      this.exports.lungo_wasm_buffer_delete(buf);
      this.free(ptr, input.length);
    }
  }

  releaseHandle(id) {
    this.exports.lungo_handle_release(id);
  }

  cloneHandle(id) {
    return this.exports.lungo_handle_clone(id);
  }

  /** Calls entry point `symbol` with type arguments and arguments (`[type, value]` pairs). */
  invoke(symbol, typeArgs, args, returns) {
    const w = new Writer(this);
    try {
      w.u32(typeArgs.length);
      for (const t of typeArgs) w.raw(t.expr);
      for (const [t, v] of args) t.encode(w, v);
      const entry = this.exports[symbol];
      const { status, out } = this.withBuffers(w.finish(), (p, n, b) => entry(p, n, b));
      return this.result(status, out, returns);
    } finally {
      for (const id of w.temps) this.release(id);
    }
  }

  result(status, out, returns) {
    if (status === 1) throw malformed(new TextDecoder().decode(out));
    if (status !== 0) throw new Error(`lungo: a generated entry point returned status ${status}`);
    const r = new Reader(out, this);
    let v;
    let failure = null;
    try {
      if (returns.kind === "value") {
        v = returns.value.decode(r);
      } else {
        const tag = r.u8();
        if (tag === 0) v = returns.value.decode(r);
        else if (tag === 1 && returns.kind === "io") {
          const id = r.u64();
          failure = new LeanIOError(r.text(), new Opaque(this, id));
        } else if (tag === 1) failure = new LeanError(returns.error.decode(r));
        else throw malformed(`invalid result tag ${tag}`);
      }
      r.finish();
    } catch (e) {
      if (e instanceof MalformedError) throw new Error(`lungo: the runtime produced a malformed result: ${e.message}`);
      throw e;
    }
    if (failure) throw failure;
    return v;
  }

  callClosure(handle, type, params, result, args) {
    if (args.length !== params.length) throw malformed(`${args.length} arguments for ${params.length} parameters`);
    const w = new Writer(this);
    try {
      w.u32(0);
      params.forEach((p, i) => p.encode(w, args[i]));
      const id = handle.live();
      const expr = this.copyIn(type.expr);
      try {
        const { status, out } = this.withBuffers(w.finish(), (p, n, b) =>
          this.exports.lungo_closure_call(this.typesPtr, id, expr, type.expr.length, p, n, b),
        );
        return this.result(status, out, value(result));
      } finally {
        this.free(expr, type.expr.length);
      }
    } finally {
      for (const id of w.temps) this.release(id);
    }
  }

  /** Registers `call` (decoding its arguments, encoding its result) for the writer's call. */
  hostFunction(w, call) {
    const id = this.register(call);
    if (!w.result) w.temps.push(id);
    return id;
  }

  register(call) {
    this.nextHost += 1n;
    this.hosts.set(this.nextHost, { call, refs: 1 });
    return this.nextHost;
  }

  retain(id) {
    const e = this.hosts.get(id);
    if (!e) throw new Error(`lungo: the runtime retained host function ${id}, which is not registered`);
    e.refs += 1;
  }

  release(id) {
    const e = this.hosts.get(id);
    if (!e) throw new Error(`lungo: host function ${id} is released more often than it is referenced`);
    e.refs -= 1;
    if (e.refs === 0) this.hosts.delete(id);
  }

  dispatch(callback, input, len, out) {
    const e = this.hosts.get(callback);
    if (!e) throw new Error(`lungo: the runtime called host function ${callback}, which is not registered`);
    let status = 0;
    let payload;
    try {
      const r = new Reader(this.memory().slice(input, input + len), this);
      const w = new Writer(this, true);
      e.call(r, w);
      r.finish();
      payload = w.finish();
    } catch (err) {
      status = 1;
      payload = utf8.encode(String(err?.message ?? err));
    }
    if (payload.length > 0) {
      const dst = this.exports.lungo_buffer_alloc(out, payload.length);
      this.memory().set(payload, dst);
    }
    return status;
  }

  /** Implements host extern `index` with `f` through `setSymbol` (`<prefix>set_host_extern`). */
  hostExtern(setSymbol, index, params, returns, f) {
    const id = this.register((r, w) => {
      const args = params.map((p) => p.decode(r));
      if (returns.kind === "value") {
        returns.value.encode(w, f(...args));
        return;
      }
      let v;
      try {
        v = f(...args);
      } catch (e) {
        if (returns.kind === "io") {
          w.u8(1);
          if (e instanceof LeanIOError && e.handle) {
            w.u64(this.cloneHandle(e.handle.live()));
            w.blob(utf8.encode(e.message));
          } else {
            // Any other exception of an IO extern is IO.userError with its message.
            w.u64(0n);
            w.blob(utf8.encode(String(e?.message ?? e)));
          }
          return;
        }
        if (e instanceof LeanError) {
          w.u8(1);
          returns.error.encode(w, e.value);
          return;
        }
        throw e;
      }
      w.u8(0);
      returns.value.encode(w, v);
    });
    this.exports[setSymbol](index, id);
  }

  /** Runs the program's `main` (`symbol`) with `args`; its exit code. */
  runMain(symbol, args) {
    const strings = args.map((a, i) => {
      if (typeof a !== "string" || a.includes("\0") || !a.isWellFormed()) throw malformed(`argument ${i} is not a string without NUL`);
      const b = utf8.encode(a);
      const z = new Uint8Array(b.length + 1);
      z.set(b);
      return z;
    });
    const ptrs = strings.map((s) => this.copyIn(s));
    const argv = new Uint8Array(4 * Math.max(ptrs.length, 1));
    const dv = new DataView(argv.buffer);
    ptrs.forEach((p, i) => dv.setUint32(4 * i, p, true));
    const argvPtr = this.copyIn(argv);
    try {
      return this.exports[symbol](args.length, argvPtr);
    } finally {
      this.free(argvPtr, argv.length);
      ptrs.forEach((p, i) => this.free(p, strings[i].length));
    }
  }
}
