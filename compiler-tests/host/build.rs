fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        .rust_extern("host_lookup", "crate::host::lookup")
        .rust_extern("host_log", "crate::host::log")
        .rust_extern("host_transform", "crate::host::transform")
        .compile_lean("lean")
}
