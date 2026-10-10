fn main() -> lungo_build::Result<()> {
    // Both sorts must carry a proved claim: the build fails if one is removed or rests on `sorry`.
    lungo_build::configure().require_claims("Sorting.sort").require_claims("Sorting.rank").compile_lean("lean")
}
