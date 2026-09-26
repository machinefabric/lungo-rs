//! Runs one conformance program, compiled from Lean by patina, with the given arguments.

include!(concat!(env!("OUT_DIR"), "/programs.rs"));

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(name) = args.next() else {
        eprintln!("usage: conformance PROGRAM [ARGS...]");
        std::process::exit(2);
    };
    match run(&name, args.collect()) {
        Some(code) => std::process::exit(code),
        None => {
            eprintln!("unknown conformance program {name:?}");
            std::process::exit(2);
        }
    }
}
