namespace Unknown

/-- Provided by the host application; no mapping is configured. -/
@[extern "provider_send"]
opaque providerSend : Nat → Nat

def relay (n : Nat) : Nat := providerSend (n + 1)

end Unknown
