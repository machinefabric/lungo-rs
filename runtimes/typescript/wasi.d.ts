import type { Wasi } from "./index.js";

/** A minimal WASI (preview 1) for browsers: standard output and error, clocks, randomness, empty
 * arguments and environment; no file system. */
export class BrowserWasi implements Wasi {
  constructor(options?: { stdout?: (bytes: Uint8Array) => void; stderr?: (bytes: Uint8Array) => void });
  readonly wasiImport: WebAssembly.ModuleImports;
  initialize(instance: WebAssembly.Instance): void;
}
