namespace TypeError

def fine (n : Nat) : Nat := n + 1

def broken (n : Nat) : Nat := n ++ "text"

end TypeError
