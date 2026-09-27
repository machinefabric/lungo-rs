import Foundation
import XCTest

@testable import LungoKit

/// The shared wire-format vectors (compiler-tests/wire/vectors.json), which every language's
/// support library must encode and decode exactly as the runtime does.
final class WireTests: XCTestCase {
    struct Vectors: Decodable {
        struct Valid: Decodable {
            let name: String
            let type: String
            let bytes: String
        }
        let valid: [Valid]
        let invalid: [Valid]
    }

    /// A type of JSON-represented values over a LungoKit descriptor, so the vectors exercise the
    /// descriptors.
    struct Dyn {
        let encode: (inout LungoWriter, Any) throws -> Void
        let decode: (inout LungoReader) throws -> Any
        var lifted: LungoType<Any> { LungoType<Any>(expr: [], encode: encode, decode: decode) }
    }

    static func lift<T>(_ t: LungoType<T>, _ toSwift: @escaping (Any) throws -> T, _ toJSON: @escaping (T) -> Any) -> Dyn {
        Dyn(encode: { w, v in try t.encode(&w, try toSwift(v)) }, decode: { r in toJSON(try t.decode(&r)) })
    }

    static func hex(_ s: String) -> [UInt8] {
        var out: [UInt8] = []
        var i = s.startIndex
        while i < s.endIndex {
            let j = s.index(i, offsetBy: 2)
            out.append(UInt8(s[i..<j], radix: 16)!)
            i = j
        }
        return out
    }

    static func hexString(_ b: [UInt8]) -> String { b.map { String(format: "%02x", $0) }.joined() }

    static func number(_ v: Any) -> Int64 { (v as! NSNumber).int64Value }

    static func bits(_ v: Any) -> UInt64 { UInt64((v as! [String: Any])["bits"] as! String, radix: 16)! }

    static func typeOf(_ r: inout LungoReader) throws -> Dyn? {
        let id: (Any) -> Any = { $0 }
        switch try r.u8() {
        case 0: return lift(Lungo.nat, { LungoNat($0 as! String)! }, { $0.description })
        case 1: return lift(Lungo.int, { LungoInt($0 as! String)! }, { $0.description })
        case 2: return lift(Lungo.bool, { $0 as! Bool }, id)
        case 3: return lift(Lungo.uint8, { UInt8(number($0)) }, { NSNumber(value: $0) })
        case 4: return lift(Lungo.uint16, { UInt16(number($0)) }, { NSNumber(value: $0) })
        case 5: return lift(Lungo.uint32, { UInt32(number($0)) }, { NSNumber(value: $0) })
        case 6: return lift(Lungo.uint64, { UInt64($0 as! String)! }, { String($0) })
        case 7: return lift(Lungo.usize, { UInt64($0 as! String)! }, { String($0) })
        case 8: return lift(Lungo.int8, { Int8(number($0)) }, { NSNumber(value: $0) })
        case 9: return lift(Lungo.int16, { Int16(number($0)) }, { NSNumber(value: $0) })
        case 10: return lift(Lungo.int32, { Int32(number($0)) }, { NSNumber(value: $0) })
        case 11: return lift(Lungo.int64, { Int64($0 as! String)! }, { String($0) })
        case 12: return lift(Lungo.isize, { Int64($0 as! String)! }, { String($0) })
        case 13:
            return lift(Lungo.float, { Double(bitPattern: bits($0)) }, { ["bits": String(format: "%016llx", $0.bitPattern)] })
        case 14:
            return lift(Lungo.float32, { Float(bitPattern: UInt32(bits($0))) }, { ["bits": String(format: "%08x", $0.bitPattern)] })
        case 15: return lift(Lungo.char, { ($0 as! String).unicodeScalars.first! }, { String($0) })
        case 16: return lift(Lungo.string, { $0 as! String }, id)
        case 17: return lift(Lungo.unit, { _ in LungoUnit() }, { _ in NSNull() })
        case 18:
            return lift(Lungo.byteArray, { hex(($0 as! [String: Any])["bytes"] as! String) }, { ["bytes": hexString($0)] })
        case 19:
            return lift(
                Lungo.floatArray, { ($0 as! [String]).map { Double(bitPattern: UInt64($0, radix: 16)!) } },
                { $0.map { String(format: "%016llx", $0.bitPattern) } })
        case 20:
            guard let inner = try typeOf(&r) else { return nil }
            return lift(
                Lungo.option(inner.lifted),
                { v in
                    let m = v as! [String: Any]
                    return m.keys.contains("some") ? .some(m["some"]!) : .none
                }, { $0.map { ["some": $0] } ?? ["none": NSNull()] })
        case 21, 22:
            let tag = r.data[r.pos - 1]
            guard let inner = try typeOf(&r) else { return nil }
            return lift(tag == 21 ? Lungo.list(inner.lifted) : Lungo.array(inner.lifted), { $0 as! [Any] }, { $0 })
        case 23:
            guard let a = try typeOf(&r), let b = try typeOf(&r) else { return nil }
            return lift(Lungo.pair(a.lifted, b.lifted), { v in let xs = v as! [Any]; return (xs[0], xs[1]) }, { [$0.0, $0.1] })
        case 24:
            guard let e = try typeOf(&r), let a = try typeOf(&r) else { return nil }
            return lift(
                Lungo.except(e.lifted, a.lifted),
                { v in
                    let m = v as! [String: Any]
                    return m.keys.contains("ok") ? .ok(m["ok"]!) : .error(m["error"]!)
                },
                { x in
                    switch x {
                    case .ok(let v): return ["ok": v]
                    case .error(let e): return ["error": e]
                    }
                })
        case 25:
            // Function values are handles and host callbacks naming live objects of a running
            // program: their rejection is checked by `invalidVectorsAreRejected`.
            let n = try r.u32()
            for _ in 0...n { _ = try typeOf(&r) }
            return nil
        case 28:
            return nil
        case let t:
            XCTFail("unknown type tag \(t)")
            return nil
        }
    }

    func vectors() throws -> Vectors {
        let url = Bundle.module.url(forResource: "vectors", withExtension: "json")!
        return try JSONDecoder().decode(Vectors.self, from: Data(contentsOf: url))
    }

    func values() throws -> [[String: Any]] {
        let url = Bundle.module.url(forResource: "vectors", withExtension: "json")!
        let json = try JSONSerialization.jsonObject(with: Data(contentsOf: url)) as! [String: Any]
        return json["valid"] as! [[String: Any]]
    }

    func canonical(_ v: Any) throws -> String {
        let data = try JSONSerialization.data(withJSONObject: ["v": v], options: [.sortedKeys, .fragmentsAllowed])
        return String(decoding: data, as: UTF8.self)
    }

    func testValidVectorsRoundTrip() throws {
        var checked = 0
        for v in try values() {
            var tr = LungoReader(WireTests.hex(v["type"] as! String), program: nil)
            guard let d = try WireTests.typeOf(&tr) else { continue }
            var w = LungoWriter(program: nil, result: false)
            try d.encode(&w, v["value"]!)
            XCTAssertEqual(WireTests.hexString(w.bytes), v["bytes"] as! String, v["name"] as! String)
            var r = LungoReader(WireTests.hex(v["bytes"] as! String), program: nil)
            let back = try d.decode(&r)
            try r.finish()
            XCTAssertEqual(try canonical(back), try canonical(v["value"]!), v["name"] as! String)
            checked += 1
        }
        XCTAssertGreaterThan(checked, 40)
    }

    func testInvalidVectorsAreRejected() throws {
        for v in try vectors().invalid {
            var tr = LungoReader(WireTests.hex(v.type), program: nil)
            var r = LungoReader(WireTests.hex(v.bytes), program: nil)
            var rejected = false
            do {
                if let d = try WireTests.typeOf(&tr) {
                    _ = try d.decode(&r)
                    try r.finish()
                } else {
                    // A function: its kind byte must be valid.
                    _ = try r.leanFunction()
                }
            } catch is LungoMalformed {
                rejected = true
            }
            XCTAssertTrue(rejected, v.name)
        }
    }

    func testNumbers() {
        let big = LungoNat("1606938044258990275541962092341162602522202993782792835301376")!
        XCTAssertEqual(big.description, "1606938044258990275541962092341162602522202993782792835301376")
        XCTAssertNil(big.uint64)
        XCTAssertEqual(LungoNat(UInt64.max).uint64, UInt64.max)
        XCTAssertNil(LungoNat("12a"))
        XCTAssertEqual(LungoInt("-9223372036854775808")!.int64, Int64.min)
        XCTAssertNil(LungoInt("-9223372036854775809")!.int64)
        XCTAssertLessThan(LungoInt(-5), LungoInt(3))
        XCTAssertLessThan(LungoNat(5), big)
    }
}
