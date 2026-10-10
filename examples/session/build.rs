fn main() -> lungo_build::Result<()> {
    // `step` and `run` must each carry a proved claim: the build fails if one is removed or
    // rests on `sorry`.
    lungo_build::configure().require_claims("Formal.step").require_claims("Formal.run").compile_lean("lean")
}
