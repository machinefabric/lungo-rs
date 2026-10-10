fn main() -> lungo_build::Result<()> {
    lungo_build::configure().require_claims("Store.copy").require_claims("Store.swap").compile_lean("lean")
}
