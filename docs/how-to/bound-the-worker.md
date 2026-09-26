# How to bound the resources a build may use

The lungo worker runs Lean code of your project and its dependencies during the build.
This guide shows how to limit its time and memory and isolate it from your environment, for
example in CI.

## Limit time

To stop a build that runs too long, set a wall-clock limit:

```rust
use std::time::Duration;

fn main() -> lungo_build::Result<()> {
    lungo_build::configure().worker_timeout(Duration::from_secs(600)).compile_lean("lean")
}
```

When the limit passes, the worker and every process it started are killed and the build fails
with [`LNG0304`](../reference/errors.md#ptn0304).

To bound processor time instead (a busy loop, rather than a slow machine), use
`.worker_cpu_limit(Duration::from_secs(300))`; exceeding it fails with
[`LNG0305`](../reference/errors.md#ptn0305). On Unix the limit applies to each process of the
worker; on Windows to all of them together.

## Limit memory

```rust
.worker_memory_limit(8 << 30) // 8 GiB
```

The limit is enforced on Linux (address space of each worker process) and Windows (memory of
the worker's processes together). Lean maps the compiled files of every imported module into
memory, so leave room above what `lake build` needs. On other systems, such as macOS, setting
it fails with [`LNG0104`](../reference/errors.md#ptn0104) rather than being ignored; set it
only for the platforms that enforce it:

```rust
let mut config = lungo_build::configure();
if cfg!(any(target_os = "linux", windows)) {
    config = config.worker_memory_limit(8 << 30);
}
config.compile_lean("lean")
```

(`cfg!` in a build script describes the host, which is where the worker runs.)

## Isolate the environment

`.hermetic(true)` runs the worker with a minimal environment (paths, home, temporary
directories, locale, elan) instead of yours, so variables such as `LEAN_PATH` or credentials
never reach Lean code. `.hermetic_worker_cache(true)` builds the worker inside the build
output instead of the shared cache in your home directory.

These limits contain runaway builds. They do not sandbox file-system or network access; see
[Trust and verification](../explanation/trust-and-verification.md).
