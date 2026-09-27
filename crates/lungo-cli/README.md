# lungo-cli

The `lungo` command: generates code from a Lean program for C, Go, Python, Swift (and
Objective-C), TypeScript and Rust, as `protoc` does from `.proto` files.

```sh
cargo install lungo-cli
lungo generate --go_out=gen/go --python_out=gen/py --ts_out=gen/js
```

Every language runs the program on the lungo runtime, prebuilt for each platform and
published with each release; the generated packages download it and check its SHA-256
digest. See the [documentation](https://jowharshamshiri.github.io/lungo/docs) and the
[command reference](https://jowharshamshiri.github.io/lungo/docs/reference/lungo-cli).
