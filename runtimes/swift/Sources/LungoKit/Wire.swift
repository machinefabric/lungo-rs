/// Encodes values in the wire format. `result`: the values are a result (their handles and host
/// functions are given to the runtime) rather than arguments (lent for the call).
public struct LungoWriter {
    public internal(set) var bytes: [UInt8] = []
    let program: LungoProgram?
    let result: Bool
    /// Host functions registered for the call, released when it returns.
    var temps: [UInt64] = []

    init(program: LungoProgram?, result: Bool) {
        self.program = program
        self.result = result
    }

    public mutating func u8(_ v: UInt8) { bytes.append(v) }
    public mutating func u16(_ v: UInt16) { withUnsafeBytes(of: v.littleEndian) { bytes.append(contentsOf: $0) } }
    public mutating func u32(_ v: UInt32) { withUnsafeBytes(of: v.littleEndian) { bytes.append(contentsOf: $0) } }
    public mutating func u64(_ v: UInt64) { withUnsafeBytes(of: v.littleEndian) { bytes.append(contentsOf: $0) } }
    public mutating func f64(_ v: Double) { u64(v.bitPattern) }
    public mutating func f32(_ v: Float) { u32(v.bitPattern) }

    /// A length or count, which the wire format limits to 32 bits.
    public mutating func length(_ n: Int) throws {
        guard n <= Int(UInt32.max) else { throw LungoMalformed("a length of \(n) exceeds the wire format's limit") }
        u32(UInt32(n))
    }

    public mutating func blob(_ b: [UInt8]) throws {
        try length(b.count)
        bytes.append(contentsOf: b)
    }

    public mutating func raw(_ b: [UInt8]) { bytes.append(contentsOf: b) }
}

/// Decodes values the runtime produced.
public struct LungoReader {
    let data: [UInt8]
    var pos = 0
    let program: LungoProgram?

    init(_ data: [UInt8], program: LungoProgram?) {
        self.data = data
        self.program = program
    }

    mutating func take(_ n: Int) throws -> ArraySlice<UInt8> {
        guard n >= 0, n <= data.count - pos else {
            throw LungoMalformed("wire data ends after \(data.count) bytes; \(n) more expected")
        }
        defer { pos += n }
        return data[pos..<pos + n]
    }

    private mutating func fixed<T: FixedWidthInteger>(_: T.Type) throws -> T {
        let b = try take(MemoryLayout<T>.size)
        var v: T = 0
        for (i, x) in b.enumerated() { v |= T(truncatingIfNeeded: x) << (8 * i) }
        return v
    }

    public mutating func u8() throws -> UInt8 { try fixed(UInt8.self) }
    public mutating func u16() throws -> UInt16 { try fixed(UInt16.self) }
    public mutating func u32() throws -> UInt32 { try fixed(UInt32.self) }
    public mutating func u64() throws -> UInt64 { try fixed(UInt64.self) }
    public mutating func f64() throws -> Double { Double(bitPattern: try u64()) }
    public mutating func f32() throws -> Float { Float(bitPattern: try u32()) }

    /// A length or count of items of at least `unit` bytes, which must fit the rest.
    public mutating func count(unit: Int = 1) throws -> Int {
        let n = Int(try u32())
        guard n * max(unit, 1) <= data.count - pos else {
            throw LungoMalformed("a length of \(n) exceeds the remaining wire data")
        }
        return n
    }

    public mutating func blob() throws -> [UInt8] { Array(try take(try count())) }

    public mutating func text() throws -> String {
        let b = try blob()
        guard let s = decodeUTF8(b) else { throw LungoMalformed("a string is not valid UTF-8") }
        return s
    }

    public func finish() throws {
        guard pos == data.count else { throw LungoMalformed("\(data.count - pos) unexpected bytes after the wire data") }
    }
}

/// The string of UTF-8 `bytes`, or `nil` if they are not valid UTF-8.
func decodeUTF8(_ bytes: [UInt8]) -> String? {
    var out = ""
    var it = bytes.makeIterator()
    var decoder = UTF8()
    while true {
        switch decoder.decode(&it) {
        case .scalarValue(let s): out.unicodeScalars.append(s)
        case .emptyInput: return out
        case .error: return nil
        }
    }
}
