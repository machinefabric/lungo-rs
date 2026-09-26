namespace FalseProof

def double (n : Nat) : Nat := n + n

theorem double_zero : double 0 = 0 := by decide

theorem double_one_wrong : double 1 = 3 := by decide

end FalseProof
