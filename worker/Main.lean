import Patina.Driver

def main (args : List String) : IO UInt32 := do
  match args with
  | [request, response] => Patina.Driver.main request response
  | _ =>
    IO.eprintln "usage: patina-worker REQUEST_FRAME RESPONSE_FRAME"
    return 2
