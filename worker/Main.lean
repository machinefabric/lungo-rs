import Lean2Rust.Driver

def main (args : List String) : IO UInt32 := do
  match args with
  | [request, response] => Lean2Rust.Driver.main request response
  | _ =>
    IO.eprintln "usage: lean2rust-worker REQUEST_FRAME RESPONSE_FRAME"
    return 2
