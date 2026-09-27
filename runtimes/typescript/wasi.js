// A minimal WASI (preview 1) for browsers: standard output and error (to `console` or given
// writers), clocks, randomness, empty arguments and environment. Other calls fail with ENOSYS
// (file system) or EBADF (unknown descriptors), as a system without them would.

const ESUCCESS = 0;
const EBADF = 8;
const ENOSYS = 52;

class ProcExit extends Error {
  constructor(code) {
    super(`exit ${code}`);
    this.code = code;
  }
}

export class BrowserWasi {
  constructor({ stdout, stderr } = {}) {
    const decoder = new TextDecoder();
    const lineWriter = (sink) => {
      let pending = "";
      return (bytes) => {
        pending += decoder.decode(bytes, { stream: true });
        let nl;
        while ((nl = pending.indexOf("\n")) >= 0) {
          sink(pending.slice(0, nl));
          pending = pending.slice(nl + 1);
        }
      };
    };
    this.writers = {
      1: stdout ?? lineWriter((line) => console.log(line)),
      2: stderr ?? lineWriter((line) => console.error(line)),
    };
    this.memory = null;
    const view = () => new DataView(this.memory.buffer);
    const u8 = () => new Uint8Array(this.memory.buffer);
    this.wasiImport = {
      args_sizes_get: (argc, size) => {
        view().setUint32(argc, 0, true);
        view().setUint32(size, 0, true);
        return ESUCCESS;
      },
      args_get: () => ESUCCESS,
      environ_sizes_get: (count, size) => {
        view().setUint32(count, 0, true);
        view().setUint32(size, 0, true);
        return ESUCCESS;
      },
      environ_get: () => ESUCCESS,
      clock_res_get: (_id, res) => {
        view().setBigUint64(res, 1000n, true);
        return ESUCCESS;
      },
      clock_time_get: (id, _precision, time) => {
        const ns = id === 0 ? BigInt(Date.now()) * 1000000n : BigInt(Math.round(performance.now() * 1e6));
        view().setBigUint64(time, ns, true);
        return ESUCCESS;
      },
      random_get: (buf, len) => {
        crypto.getRandomValues(u8().subarray(buf, buf + len));
        return ESUCCESS;
      },
      fd_write: (fd, iovs, iovsLen, nwritten) => {
        const write = this.writers[fd];
        if (!write) return EBADF;
        let total = 0;
        for (let i = 0; i < iovsLen; i++) {
          const ptr = view().getUint32(iovs + 8 * i, true);
          const len = view().getUint32(iovs + 8 * i + 4, true);
          write(u8().slice(ptr, ptr + len));
          total += len;
        }
        view().setUint32(nwritten, total, true);
        return ESUCCESS;
      },
      fd_fdstat_get: (fd, stat) => {
        if (fd > 2) return EBADF;
        const v = view();
        v.setUint8(stat, 2); // character device
        v.setUint16(stat + 2, 0, true);
        v.setBigUint64(stat + 8, 0n, true);
        v.setBigUint64(stat + 16, 0n, true);
        return ESUCCESS;
      },
      fd_prestat_get: () => EBADF,
      fd_prestat_dir_name: () => EBADF,
      fd_close: (fd) => (fd <= 2 ? ESUCCESS : EBADF),
      proc_exit: (code) => {
        throw new ProcExit(code);
      },
      sched_yield: () => ESUCCESS,
    };
    // Every other call: not available without a file system.
    this.wasiImport = new Proxy(this.wasiImport, {
      get: (target, name) => target[name] ?? (() => ENOSYS),
    });
  }

  /** Initializes a reactor module instantiated with `wasiImport`. */
  initialize(instance) {
    this.memory = instance.exports.memory;
    instance.exports._initialize?.();
  }
}
