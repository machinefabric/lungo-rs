//! Release gates exercised by a real Cargo package: Lean code calling back into Rust, a Lean
//! project using custom syntax and macros, and a PureRust binary free of Lean's native runtime.

use host::{Token, host as callbacks};
use lungo::{LeanClosure, List, Nat};

fn nat(n: u64) -> Nat {
    Nat::from(n as u128)
}

fn nats(xs: &[u64]) -> List<Nat> {
    xs.iter().map(|&x| nat(x)).collect()
}

#[test]
fn lean_calls_back_into_rust() {
    // One test drives the host's global table and log, so the steps cannot interleave.
    callbacks::set_table(&[("alpha", 1), ("beta", 20), ("gamma", 300)]);
    callbacks::take_log();

    let total = host::resolve_all(["alpha", "gamma", "beta"].map(String::from).into_iter().collect());
    assert_eq!(total.map_err(|e| e.message().to_owned()), Ok(nat(321)));
    assert_eq!(callbacks::take_log(), ["alpha => 1", "gamma => 300", "beta => 20"]);

    // A missing key raises a Lean `IO` error after the earlier callbacks ran.
    let missing = host::resolve_all(["beta", "delta"].map(String::from).into_iter().collect());
    assert_eq!(missing.map_err(|e| e.message().to_owned()), Err("unknown key: delta".to_owned()));
    assert_eq!(callbacks::take_log(), ["beta => 20"]);

    // Errors raised by the Rust side of an `IO` extern reach Lean callers as `IO.Error`s.
    let err = host::host_log(String::new()).unwrap_err();
    assert_eq!(err.message(), "empty log line");
    assert_eq!(host::host_lookup("gamma".into()), Some(nat(300)));
    assert_eq!(host::host_lookup("nope".into()), None);
}

#[test]
fn closures_cross_the_boundary_in_both_directions() {
    // Lean passes its closure to Rust, which calls it back for every element.
    assert_eq!(host::transformed(nats(&[1, 2, 3, 0]), nat(10)), nats(&[11, 21, 31, 1]));
    // Rust passes its closure to Lean, which passes it back to Rust.
    let f = LeanClosure::from_fn(|n: Nat| &n * &n);
    assert_eq!(host::host_transform(f, nats(&[3, 4, 12])), nats(&[9, 16, 144]));
    // Values beyond machine integers flow through the callbacks unchanged.
    let big: Nat = "123456789012345678901234567890".parse().unwrap();
    let r = host::transformed(List::from(vec![big.clone()]), nat(1));
    assert_eq!(r, List::from(vec![&big + &nat(1)]));
}

#[test]
fn custom_syntax_and_macros_compile_unchanged() {
    // `defconst answer := sum! 20, 20, 2`
    assert_eq!(host::answer(), nat(42));
    // `sum! n, answer, (n |>> (· * 3))` = n + 42 + 9n
    for n in [0u64, 1, 7, 1 << 40] {
        assert_eq!(host::macro_demo(nat(n)), &(&nat(n) + &nat(42)) + &(&nat(n) * &nat(9)));
    }
    let toks =
        vec![Token::Num { n: nat(2) }, Token::Times, Token::Num { n: nat(3) }, Token::Plus, Token::Num { n: nat(4) }];
    assert_eq!(host::eval_tokens(List::from(toks)), nat(10));
    assert_eq!(host::repr_token(Token::Num { n: nat(5) }), "Host.Token.num 5");
    assert_eq!(host::repr_token(Token::Plus), "Host.Token.plus");
    // Derived `Hashable` is deterministic and distinguishes constructors and fields.
    assert_eq!(host::token_hash(Token::Num { n: nat(1) }), host::token_hash(Token::Num { n: nat(1) }));
    assert_ne!(host::token_hash(Token::Num { n: nat(1) }), host::token_hash(Token::Num { n: nat(2) }));
    assert_ne!(host::token_hash(Token::Plus), host::token_hash(Token::Times));
}

/// Gate: PureRust mode links no Lean runtime: the build records no native link directives.
#[test]
fn pure_rust_build_links_no_native_libraries() {
    let info = include_str!(concat!(env!("OUT_DIR"), "/lungo/build-info.json"));
    assert!(info.contains("\"link_directives\": []"), "PureRust build links native libraries:\n{info}");
}

/// Gate: PureRust mode links no Lean runtime. The binary may contain no C-level symbol of
/// Lean's runtime (`lean_*`) or of Lean-emitted C code (`l_*`, `initialize_*`): the runtime
/// and all compiled Lean code are Rust, whose symbols are mangled. (MSVC keeps symbols in a
/// separate PDB file rather than the executable, so this reading applies to ELF and Mach-O.)
#[cfg(not(all(windows, target_env = "msvc")))]
#[test]
fn pure_rust_binary_contains_no_lean_native_code() {
    use object::{Object, ObjectSymbol};
    let exe = std::env::current_exe().unwrap();
    let data = std::fs::read(&exe).unwrap();
    let file = object::File::parse(&*data).unwrap();
    let mut symbols = 0;
    let mut offenders = Vec::new();
    for sym in file.symbols().chain(file.dynamic_symbols()) {
        let Ok(name) = sym.name() else { continue };
        symbols += 1;
        // Mach-O prefixes C symbols with `_`.
        let c = name.strip_prefix('_').filter(|_| cfg!(target_vendor = "apple")).unwrap_or(name);
        if c.starts_with("lean_") || c.starts_with("l_") || c.starts_with("initialize_") {
            offenders.push(name.to_owned());
        }
    }
    assert!(symbols > 1000, "the test binary's symbol table was not read ({symbols} symbols)");
    offenders.sort();
    assert!(offenders.is_empty(), "Lean native symbols in a PureRust binary: {offenders:?}");
}
