fn main() -> lungo_build::Result<()> {
    lungo_build::configure().require_claims("Access.evaluate").require_claims("Access.observe").compile_lean("lean")
}
