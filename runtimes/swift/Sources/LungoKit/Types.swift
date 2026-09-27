/// How Swift values of `T` cross the boundary as values of a Lean type: the type's expression
/// (for type arguments and function types) and its wire encoding. Polymorphic functions of
/// generated packages take one per type parameter.
public struct LungoType<T> {
    public let expr: [UInt8]
    let encodeValue: (inout LungoWriter, T) throws -> Void
    let decodeValue: (inout LungoReader) throws -> T

    public init(
        expr: [UInt8],
        encode: @escaping (inout LungoWriter, T) throws -> Void,
        decode: @escaping (inout LungoReader) throws -> T
    ) {
        self.expr = expr
        self.encodeValue = encode
        self.decodeValue = decode
    }

    public func encode(_ w: inout LungoWriter, _ v: T) throws { try encodeValue(&w, v) }
    public func decode(_ r: inout LungoReader) throws -> T { try decodeValue(&r) }
}

/// Lean's `Unit`.
public struct LungoUnit: Hashable, Sendable {
    public init() {}
}

/// Lean's `Except ε α`.
public enum LungoExcept<E, A> {
    case error(E)
    case ok(A)
}

extension LungoExcept: Equatable where E: Equatable, A: Equatable {}

enum Tag {
    static let nat: UInt8 = 0, int: UInt8 = 1, bool: UInt8 = 2, uint8: UInt8 = 3, uint16: UInt8 = 4
    static let uint32: UInt8 = 5, uint64: UInt8 = 6, usize: UInt8 = 7, int8: UInt8 = 8, int16: UInt8 = 9
    static let int32: UInt8 = 10, int64: UInt8 = 11, isize: UInt8 = 12, float: UInt8 = 13, float32: UInt8 = 14
    static let char: UInt8 = 15, string: UInt8 = 16, unit: UInt8 = 17, byteArray: UInt8 = 18
    static let floatArray: UInt8 = 19, option: UInt8 = 20, list: UInt8 = 21, array: UInt8 = 22, prod: UInt8 = 23
    static let except: UInt8 = 24, function: UInt8 = 25, inductive: UInt8 = 27, opaque: UInt8 = 28
}

func le32(_ v: UInt32) -> [UInt8] { withUnsafeBytes(of: v.littleEndian) { Array($0) } }

func putMagnitude(_ w: inout LungoWriter, _ n: LungoNat) throws { try w.blob(n.bytes) }

func magnitude(_ r: inout LungoReader) throws -> LungoNat {
    let b = try r.blob()
    if b.last == 0 { throw LungoMalformed("a number's magnitude has a leading zero byte") }
    return LungoNat(bytes: b)
}

/// The descriptors of Lean's types.
public enum Lungo {
    public static let nat = LungoType<LungoNat>(expr: [Tag.nat], encode: { try putMagnitude(&$0, $1) }, decode: magnitude)

    public static let int = LungoType<LungoInt>(
        expr: [Tag.int],
        encode: { w, v in
            w.u8(v.isNegative ? 1 : 0)
            try putMagnitude(&w, v.magnitude)
        },
        decode: { r in
            let sign = try r.u8()
            let m = try magnitude(&r)
            switch sign {
            case 0: return LungoInt(magnitude: m, negative: false)
            case 1 where m.isZero: throw LungoMalformed("negative zero is not a canonical Int")
            case 1: return LungoInt(magnitude: m, negative: true)
            default: throw LungoMalformed("invalid Int sign \(sign)")
            }
        })

    public static let bool = LungoType<Bool>(
        expr: [Tag.bool], encode: { $0.u8($1 ? 1 : 0) },
        decode: { r in
            let b = try r.u8()
            guard b <= 1 else { throw LungoMalformed("invalid Bool \(b)") }
            return b == 1
        })

    public static let uint8 = LungoType<UInt8>(expr: [Tag.uint8], encode: { $0.u8($1) }, decode: { try $0.u8() })
    public static let uint16 = LungoType<UInt16>(expr: [Tag.uint16], encode: { $0.u16($1) }, decode: { try $0.u16() })
    public static let uint32 = LungoType<UInt32>(expr: [Tag.uint32], encode: { $0.u32($1) }, decode: { try $0.u32() })
    public static let uint64 = LungoType<UInt64>(expr: [Tag.uint64], encode: { $0.u64($1) }, decode: { try $0.u64() })
    /// `USize`, as a `UInt64` (the runtime rejects values its platform's `USize` cannot hold).
    public static let usize = LungoType<UInt64>(expr: [Tag.usize], encode: { $0.u64($1) }, decode: { try $0.u64() })
    public static let int8 = LungoType<Int8>(
        expr: [Tag.int8], encode: { $0.u8(UInt8(bitPattern: $1)) }, decode: { Int8(bitPattern: try $0.u8()) })
    public static let int16 = LungoType<Int16>(
        expr: [Tag.int16], encode: { $0.u16(UInt16(bitPattern: $1)) }, decode: { Int16(bitPattern: try $0.u16()) })
    public static let int32 = LungoType<Int32>(
        expr: [Tag.int32], encode: { $0.u32(UInt32(bitPattern: $1)) }, decode: { Int32(bitPattern: try $0.u32()) })
    public static let int64 = LungoType<Int64>(
        expr: [Tag.int64], encode: { $0.u64(UInt64(bitPattern: $1)) }, decode: { Int64(bitPattern: try $0.u64()) })
    /// `ISize`, as an `Int64`.
    public static let isize = LungoType<Int64>(
        expr: [Tag.isize], encode: { $0.u64(UInt64(bitPattern: $1)) }, decode: { Int64(bitPattern: try $0.u64()) })
    public static let float = LungoType<Double>(expr: [Tag.float], encode: { $0.f64($1) }, decode: { try $0.f64() })
    public static let float32 = LungoType<Float>(expr: [Tag.float32], encode: { $0.f32($1) }, decode: { try $0.f32() })

    public static let char = LungoType<Unicode.Scalar>(
        expr: [Tag.char], encode: { $0.u32($1.value) },
        decode: { r in
            let c = try r.u32()
            guard let s = Unicode.Scalar(c) else { throw LungoMalformed("\(c) is not a Unicode scalar value") }
            return s
        })

    public static let string = LungoType<String>(
        expr: [Tag.string], encode: { try $0.blob(Array($1.utf8)) }, decode: { try $0.text() })

    public static let unit = LungoType<LungoUnit>(expr: [Tag.unit], encode: { _, _ in }, decode: { _ in LungoUnit() })

    public static let byteArray = LungoType<[UInt8]>(
        expr: [Tag.byteArray], encode: { try $0.blob($1) }, decode: { try $0.blob() })

    public static let floatArray = LungoType<[Double]>(
        expr: [Tag.floatArray],
        encode: { w, v in
            try w.length(v.count)
            for x in v { w.f64(x) }
        },
        decode: { r in
            let n = try r.count(unit: 8)
            return try (0..<n).map { _ in try r.f64() }
        })

    public static let opaque = LungoType<LungoOpaque>(
        expr: [Tag.opaque],
        encode: { w, v in w.u64(try w.result ? v.handle.clone() : v.handle.live()) },
        decode: { r in LungoOpaque(LungoHandle(try r.u64())) })

    /// `Option α`.
    public static func option<A>(_ a: LungoType<A>) -> LungoType<A?> {
        LungoType<A?>(
            expr: [Tag.option] + a.expr,
            encode: { w, v in
                if let v {
                    w.u8(1)
                    try a.encode(&w, v)
                } else {
                    w.u8(0)
                }
            },
            decode: { r in
                switch try r.u8() {
                case 0: return .none
                case 1: return .some(try a.decode(&r))
                case let t: throw LungoMalformed("invalid Option tag \(t)")
                }
            })
    }

    private static func sequence<A>(_ tag: UInt8, _ a: LungoType<A>) -> LungoType<[A]> {
        LungoType<[A]>(
            expr: [tag] + a.expr,
            encode: { w, v in
                try w.length(v.count)
                for x in v { try a.encode(&w, x) }
            },
            decode: { r in
                let n = try r.count()
                var out: [A] = []
                out.reserveCapacity(n)
                for _ in 0..<n { out.append(try a.decode(&r)) }
                return out
            })
    }

    /// `List α`, as an array.
    public static func list<A>(_ a: LungoType<A>) -> LungoType<[A]> { sequence(Tag.list, a) }

    /// `Array α`.
    public static func array<A>(_ a: LungoType<A>) -> LungoType<[A]> { sequence(Tag.array, a) }

    /// `α × β`, as a tuple.
    public static func pair<A, B>(_ a: LungoType<A>, _ b: LungoType<B>) -> LungoType<(A, B)> {
        LungoType<(A, B)>(
            expr: [Tag.prod] + a.expr + b.expr,
            encode: { w, v in
                try a.encode(&w, v.0)
                try b.encode(&w, v.1)
            },
            decode: { r in
                let x = try a.decode(&r)
                return (x, try b.decode(&r))
            })
    }

    /// `Except ε α`.
    public static func except<E, A>(_ e: LungoType<E>, _ a: LungoType<A>) -> LungoType<LungoExcept<E, A>> {
        LungoType<LungoExcept<E, A>>(
            expr: [Tag.except] + e.expr + a.expr,
            encode: { w, v in
                switch v {
                case .error(let x):
                    w.u8(0)
                    try e.encode(&w, x)
                case .ok(let x):
                    w.u8(1)
                    try a.encode(&w, x)
                }
            },
            decode: { r in
                switch try r.u8() {
                case 0: return .error(try e.decode(&r))
                case 1: return .ok(try a.decode(&r))
                case let t: throw LungoMalformed("invalid Except tag \(t)")
                }
            })
    }

    /// The type expression of type `index` of a program's type table applied to `args`.
    public static func inductiveExpr(_ index: UInt32, _ args: [UInt8]...) -> [UInt8] {
        [Tag.inductive] + le32(index) + le32(UInt32(args.count)) + args.flatMap { $0 }
    }
}
