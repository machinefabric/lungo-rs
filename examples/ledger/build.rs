fn main() -> lungo_build::Result<()> {
    lungo_build::configure().require_claims("Ledger.replay").compile_lean("lean")
}
