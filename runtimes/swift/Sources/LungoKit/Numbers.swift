/// Lean's `Nat`: a natural number of any size.
public struct LungoNat: Hashable, Comparable, Sendable, ExpressibleByIntegerLiteral, CustomStringConvertible,
    LosslessStringConvertible
{
    /// Little-endian 32-bit limbs without trailing zeros (empty for zero).
    var limbs: [UInt32]

    init(limbs: [UInt32]) {
        var l = limbs
        while l.last == 0 { l.removeLast() }
        self.limbs = l
    }

    public init(_ v: UInt64) {
        self.init(limbs: [UInt32(truncatingIfNeeded: v), UInt32(truncatingIfNeeded: v >> 32)])
    }

    public init(integerLiteral v: UInt64) { self.init(v) }

    /// The number of decimal `digits` (at least one, digits only), or `nil`.
    public init?(_ digits: String) {
        guard !digits.isEmpty, digits.utf8.allSatisfy({ (48...57).contains($0) }) else { return nil }
        var n = LungoNat(limbs: [])
        for d in digits.utf8 {
            n = n.multipliedAdding(10, UInt32(d - 48))
        }
        self = n
    }

    /// The value, if it fits 64 bits.
    public var uint64: UInt64? {
        switch limbs.count {
        case 0: return 0
        case 1: return UInt64(limbs[0])
        case 2: return UInt64(limbs[0]) | UInt64(limbs[1]) << 32
        default: return nil
        }
    }

    public var isZero: Bool { limbs.isEmpty }

    func multipliedAdding(_ m: UInt32, _ a: UInt32) -> LungoNat {
        var out: [UInt32] = []
        out.reserveCapacity(limbs.count + 1)
        var carry = UInt64(a)
        for l in limbs {
            let x = UInt64(l) * UInt64(m) + carry
            out.append(UInt32(truncatingIfNeeded: x))
            carry = x >> 32
        }
        if carry != 0 { out.append(UInt32(carry)) }
        return LungoNat(limbs: out)
    }

    /// The quotient and remainder of a division by `d` (non-zero).
    func dividedBy(_ d: UInt32) -> (LungoNat, UInt32) {
        var out = [UInt32](repeating: 0, count: limbs.count)
        var rem: UInt64 = 0
        for i in stride(from: limbs.count - 1, through: 0, by: -1) {
            let x = rem << 32 | UInt64(limbs[i])
            out[i] = UInt32(x / UInt64(d))
            rem = x % UInt64(d)
        }
        return (LungoNat(limbs: out), UInt32(rem))
    }

    public var description: String {
        if limbs.isEmpty { return "0" }
        var chunks: [UInt32] = []
        var n = self
        while !n.isZero {
            let (q, r) = n.dividedBy(1_000_000_000)
            chunks.append(r)
            n = q
        }
        var s = String(chunks.last!)
        for c in chunks.dropLast().reversed() {
            let part = String(c)
            s += String(repeating: "0", count: 9 - part.count) + part
        }
        return s
    }

    public static func < (a: LungoNat, b: LungoNat) -> Bool {
        if a.limbs.count != b.limbs.count { return a.limbs.count < b.limbs.count }
        for i in stride(from: a.limbs.count - 1, through: 0, by: -1) where a.limbs[i] != b.limbs[i] {
            return a.limbs[i] < b.limbs[i]
        }
        return false
    }

    /// The magnitude's little-endian bytes, without trailing zeros.
    var bytes: [UInt8] {
        var out: [UInt8] = []
        for l in limbs {
            for k in 0..<4 { out.append(UInt8(truncatingIfNeeded: l >> (8 * UInt32(k)))) }
        }
        while out.last == 0 { out.removeLast() }
        return out
    }

    init(bytes: [UInt8]) {
        var limbs = [UInt32](repeating: 0, count: (bytes.count + 3) / 4)
        for (i, b) in bytes.enumerated() {
            limbs[i / 4] |= UInt32(b) << (8 * UInt32(i % 4))
        }
        self.init(limbs: limbs)
    }
}

/// Lean's `Int`: an integer of any size.
public struct LungoInt: Hashable, Comparable, Sendable, ExpressibleByIntegerLiteral, CustomStringConvertible,
    LosslessStringConvertible
{
    public let magnitude: LungoNat
    /// Whether the integer is negative (never for zero).
    public let isNegative: Bool

    public init(magnitude: LungoNat, negative: Bool) {
        self.magnitude = magnitude
        self.isNegative = negative && !magnitude.isZero
    }

    public init(_ v: Int64) {
        self.init(magnitude: LungoNat(v.magnitude), negative: v < 0)
    }

    public init(integerLiteral v: Int64) { self.init(v) }

    /// The integer of decimal `digits` with an optional leading `-`, or `nil`.
    public init?(_ digits: String) {
        let negative = digits.hasPrefix("-")
        guard let m = LungoNat(negative ? String(digits.dropFirst()) : digits) else { return nil }
        self.init(magnitude: m, negative: negative)
    }

    /// The value, if it fits 64 bits.
    public var int64: Int64? {
        guard let m = magnitude.uint64 else { return nil }
        if isNegative { return m <= UInt64(Int64.max) + 1 ? Int64(truncatingIfNeeded: 0 &- m) : nil }
        return m <= UInt64(Int64.max) ? Int64(m) : nil
    }

    public var description: String { (isNegative ? "-" : "") + magnitude.description }

    public static func < (a: LungoInt, b: LungoInt) -> Bool {
        switch (a.isNegative, b.isNegative) {
        case (true, false): return true
        case (false, true): return false
        case (false, false): return a.magnitude < b.magnitude
        case (true, true): return b.magnitude < a.magnitude
        }
    }
}
