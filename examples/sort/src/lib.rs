//! Merge sort with a contract, compiled from Lean: `sort` and `rank` are proved to return a
//! sorted permutation of their input (`Sorting.sort_satisfies`, `Sorting.rank_satisfies`), and
//! `rank` to keep the order of entries with the same score (`Sorting.rank_stable`).

lungo::include_lean!("sorting");
