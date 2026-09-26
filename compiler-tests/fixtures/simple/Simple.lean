module

public def twice (n : Nat) : Nat := n + n

public theorem twice_zero : twice 0 = 0 := by decide

@[extern "provider_send"]
public opaque providerSend : Nat → Nat
