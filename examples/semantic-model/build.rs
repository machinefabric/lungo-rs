fn main() -> lungo_build::Result<()> {
    lungo_build::configure()
        .require_claims("Inventory.lookup")
        .require_claims("Inventory.lookupSteps")
        .compile_lean("lean")
}
