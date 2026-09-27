//! Runs one conformance program, compiled from Lean by lungo's Rust or C backend, with the
//! given arguments.

// The programs compiled by the C backend link against the runtime's C library.
use lungo_runtime as _;

include!(concat!(env!("OUT_DIR"), "/programs.rs"));

fn main() {
    let mut args = std::env::args().skip(1);
    let (Some(backend), Some(name)) = (args.next(), args.next()) else {
        eprintln!("usage: conformance rust|c PROGRAM [ARGS...]");
        std::process::exit(2);
    };
    let args: Vec<String> = args.collect();
    let code = match backend.as_str() {
        "rust" => run_rust(&name, args),
        "c" => run_c(&name, args),
        _ => {
            eprintln!("unknown backend {backend:?}: expected rust or c");
            std::process::exit(2);
        }
    };
    match code {
        Some(code) => std::process::exit(code),
        None => {
            eprintln!("unknown conformance program {name:?}");
            std::process::exit(2);
        }
    }
}
