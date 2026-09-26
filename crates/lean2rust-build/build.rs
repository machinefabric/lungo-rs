fn main() {
    // lean2rust-build runs on the build host, so its own compilation target is the host triple.
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for build scripts");
    println!("cargo::rustc-env=LEAN2RUST_BUILD_HOST={target}");
}
