fn main() {
    // lungo-driver runs on the build host, so its own compilation target is the host triple.
    let target = std::env::var("TARGET").expect("Cargo sets TARGET for build scripts");
    println!("cargo::rustc-env=LUNGO_DRIVER_HOST={target}");
}
