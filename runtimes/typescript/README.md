# lungo-ts

The support library of the TypeScript/JavaScript packages [lungo](https://github.com/machinefabric/lungo)
generates from Lean programs: the wire format between JavaScript and a program's WebAssembly
module, the host functions the program calls, and the Lean values JavaScript has no type for.

Generated packages depend on exactly the `lungo-ts` of the lungo release that generated them. On
Node.js programs run on `node:wasi`; in browsers on the minimal WASI of `wasi.js` (standard output
and error, clocks, randomness; no file system).
