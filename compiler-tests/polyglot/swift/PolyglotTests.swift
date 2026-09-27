// The polyglot assertions, in Swift, on the generated Swift package.
import Foundation
import LungoKit
import Polyglot
import XCTest

final class RecordingHost: PolyglotHost {
    private let lock = NSLock()
    private(set) var lines: [String] = []

    func hostScale(_ n: LungoNat) throws -> LungoNat {
        LungoNat(n.uint64! * 10)
    }

    func hostRecord(_ line: String) throws {
        if line == "fail" { throw LungoIOError("the host refuses to record `fail`") }
        lock.lock()
        defer { lock.unlock() }
        lines.append(line)
    }

    func clear() {
        lock.lock()
        defer { lock.unlock() }
        lines = []
    }
}

let host: RecordingHost = {
    let h = RecordingHost()
    setHost(h)
    return h
}()

final class PolyglotTests: XCTestCase {
    override func setUp() {
        _ = host
    }

    func testNumbers() throws {
        XCTAssertEqual(try factorial(25).description, "15511210043330985984000000")
        let big = LungoInt("-123456789012345678901234567890")!
        XCTAssertEqual(try negate(big).description, "123456789012345678901234567890")
    }

    func testStructuresAndInductives() throws {
        let p = Point(x: 1.5, y: -2, label: "p", tag: 7)
        let moved = try moveBy(p, 1, 0.5)
        XCTAssertEqual(moved, Point(x: 2.5, y: -1.5, label: "p", tag: 7))
        XCTAssertEqual(try describe(moved), "p#7 at (2.500000, -1.500000)")
        XCTAssertEqual(try area(.rect(corner: moved, width: 3, height: 4)), 12)
        XCTAssertEqual(try area(.circle(center: p, radius: 2)), 12)
        XCTAssertEqual(try area(.empty), 0)
    }

    func testScalarsAndContainers() throws {
        XCTAssertEqual(try mix(.max, -5, "λ", true), "18446744073709551615/-5/λ/true")
        XCTAssertEqual(try reverseBytes([1, 2, 3]), [3, 2, 1])
        XCTAssertEqual(try sumFloats([0.5, 0.25, 2]), 2.75)
        XCTAssertEqual(try firstWord("hello world"), "hello")
        XCTAssertNil(try firstWord(""))
        let swapped = try swap(("x", 9))
        XCTAssertEqual(swapped.0, 9)
        XCTAssertEqual(swapped.1, "x")
        XCTAssertEqual(try evens(10), [0, 2, 4, 6, 8])
        XCTAssertEqual(try divide(10, 0), .error("division by zero"))
        XCTAssertEqual(try divide(10, 3), .ok(3))
    }

    func testErrors() throws {
        XCTAssertThrowsError(try checkedDiv(10, 0)) { e in
            XCTAssertEqual((e as? LungoIOError)?.message, "checkedDiv: division by zero")
        }
        XCTAssertThrowsError(try parseDigit("x")) { e in
            XCTAssertEqual((e as? LungoError<String>)?.value, "not a digit: x")
        }
        XCTAssertEqual(try parseDigit("7"), 7)
    }

    func testPolymorphism() throws {
        let tree = try ofList(Lungo.nat, [1, 2, 3])
        XCTAssertEqual(try size(Lungo.nat, tree), 3)
        let mirrored = try mirror(Lungo.nat, tree)
        guard case .node = mirrored else { return XCTFail("a mirrored tree is a node") }
        XCTAssertEqual(try toList(Lungo.nat, mirrored), [3, 2, 1])
        let strings: Tree<String> = .node(left: .leaf, value: "s", right: .leaf)
        XCTAssertEqual(try size(Lungo.string, strings), 1)
        XCTAssertEqual(tree, try ofList(Lungo.nat, [1, 2, 3]))
    }

    func testFunctions() throws {
        var calls: [LungoNat] = []
        let twice = try applyTwice({ x in
            calls.append(x)
            return LungoNat(x.uint64! * 2 + 1)
        }, 3)
        XCTAssertEqual(twice, 15)
        XCTAssertEqual(calls, [3, 7])
        XCTAssertEqual(try adder(5, 3), 8)
    }

    func testOpaqueValues() throws {
        let counter = try newCounter(10)
        XCTAssertEqual(try bump(counter), 11)
        XCTAssertEqual(try bump(counter), 12)
        counter.close()
        XCTAssertThrowsError(try bump(counter)) { XCTAssert($0 is LungoMalformed) }
    }

    func testHostExterns() throws {
        XCTAssertEqual(try scaledSum([1, 2, 3]), 60)
        host.clear()
        XCTAssertEqual(try recordAll(["a", "b"]), 2)
        XCTAssertThrowsError(try recordAll(["c", "fail", "d"])) { e in
            XCTAssert((e as? LungoIOError)?.message.contains("refuses to record") == true)
        }
        XCTAssertEqual(host.lines, ["a", "b", "c"])
    }

    func testConcurrentCalls() throws {
        let results = UnsafeMutableBufferPointer<UInt64>.allocate(capacity: 16)
        defer { results.deallocate() }
        DispatchQueue.concurrentPerform(iterations: 16) { i in
            results[i] = (try? factorial(LungoNat(UInt64(i))))?.uint64 ?? 0
        }
        XCTAssertEqual(results[10], 3_628_800)
        XCTAssert(results.allSatisfy { $0 > 0 })
    }

    func testRunMain() throws {
        XCTAssertEqual(try runMain(["one", "two"]), 2)
    }
}
