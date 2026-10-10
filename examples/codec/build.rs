fn main() -> lungo_build::Result<()> {
    // Every encoder and decoder must carry a proved claim.
    lungo_build::configure()
        .require_claims("Varint.encode")
        .require_claims("Varint.decode")
        .require_claims("Varint.encodeAll")
        .require_claims("Varint.decodeAll")
        .compile_lean("lean")
}
