import Lungo.Driver

def main (args : List String) : IO UInt32 := do
  match args with
  | [request, response] => Lungo.Driver.main request response
  | _ =>
    IO.eprintln "usage: lungo-worker REQUEST_FRAME RESPONSE_FRAME"
    return 2
