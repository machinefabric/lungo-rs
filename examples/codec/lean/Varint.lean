import Lungo

/-!
LEB128 varints: a number as bytes of seven bits each, lowest first, every byte but the last with
its high bit set. The claims prove that decoding what was encoded gives the number back and the
bytes after it untouched, and that input ending inside a number is refused rather than misread.
-/
namespace Varint

def encode (n : Nat) : List UInt8 :=
  if n < 128 then [n.toUInt8] else (n % 128 + 128).toUInt8 :: encode (n / 128)
termination_by n
decreasing_by omega

/-- The number at the start of `bytes`, and the bytes after it; `none` when they end inside it. -/
def decode (bytes : List UInt8) : Option (Nat × List UInt8) :=
  match bytes with
  | [] => none
  | b :: rest =>
    if b.toNat < 128 then some (b.toNat, rest)
    else
      match decode rest with
      | some (m, r) => some (b.toNat - 128 + 128 * m, r)
      | none => none

theorem toNat_toUInt8 {n : Nat} (h : n < 256) : n.toUInt8.toNat = n := by
  simp [Nat.mod_eq_of_lt h]

@[lungo_claim "lungo.roundtrip" subject encode decode]
theorem decode_encode (n : Nat) (rest : List UInt8) : decode (encode n ++ rest) = some (n, rest) := by
  induction n using Nat.strongRecOn with
  | ind n ih =>
    rw [encode]
    split
    · simp [decode, toNat_toUInt8 (by omega : n < 256), *]
    · rename_i big
      have byte := toNat_toUInt8 (by omega : n % 128 + 128 < 256)
      simp only [List.cons_append, decode, byte, ih (n / 128) (by omega)]
      simp only [show ¬(n % 128 + 128 < 128) by omega, ite_false, Option.some.injEq, Prod.mk.injEq, and_true]
      omega

/-- Bytes that all continue a number end inside it: they are refused. -/
@[lungo_claim "lungo.law" subject decode]
theorem decode_truncated (bytes : List UInt8) (h : ∀ b ∈ bytes, 128 ≤ b.toNat) : decode bytes = none := by
  induction bytes with
  | nil => rfl
  | cons b rest ih =>
    have hb := h b (by simp)
    simp [decode, show ¬(b.toNat < 128) by omega, ih (fun x hx => h x (by simp [hx]))]

/-- What `decode` leaves is shorter than what it was given: it reads at least one byte. -/
theorem decode_shortens : ∀ {bytes : List UInt8} {n : Nat} {rest : List UInt8},
    decode bytes = some (n, rest) → rest.length < bytes.length
  | [], _, _, h => by simp [decode] at h
  | b :: tail, n, rest, h => by
    simp only [decode] at h
    split at h
    · simp only [Option.some.injEq, Prod.mk.injEq] at h
      simp [← h.2]
    · split at h
      · rename_i m r hd
        simp only [Option.some.injEq, Prod.mk.injEq] at h
        have := decode_shortens hd
        simp only [List.length_cons, ← h.2]
        omega
      · simp at h

/-- Every number in `bytes`, in order; `none` when the bytes end inside one. -/
def decodeAll (bytes : List UInt8) : Option (List Nat) :=
  match bytes with
  | [] => some []
  | b :: bs =>
    match h : decode (b :: bs) with
    | some (n, rest) =>
      have : rest.length < (b :: bs).length := decode_shortens h
      (decodeAll rest).map (n :: ·)
    | none => none
termination_by bytes.length

/-- Encodes every number in turn. -/
def encodeAll (ns : List Nat) : List UInt8 := ns.flatMap encode

theorem encode_ne_nil (n : Nat) : encode n ≠ [] := by
  rw [encode]; split <;> simp

@[lungo_claim "lungo.roundtrip" subject encodeAll decodeAll]
theorem decodeAll_encodeAll (ns : List Nat) : decodeAll (encodeAll ns) = some ns := by
  induction ns with
  | nil => simp [encodeAll, decodeAll]
  | cons n ns ih =>
    simp only [encodeAll, List.flatMap_cons] at ih ⊢
    obtain ⟨b, bs, hb⟩ := List.exists_cons_of_ne_nil (encode_ne_nil n)
    have h := decode_encode n (ns.flatMap encode)
    rw [hb] at h
    rw [hb, List.cons_append, decodeAll]
    split
    · rename_i m rest hd
      simp only [List.cons_append] at h
      rw [h] at hd
      simp only [Option.some.injEq, Prod.mk.injEq] at hd
      obtain ⟨rfl, rfl⟩ := hd
      simp [ih]
    · rename_i hd
      simp only [List.cons_append] at h
      rw [h] at hd
      simp at hd

end Varint
