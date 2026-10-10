// The polyglot assertions, in Swift, on the generated Swift package.
import Foundation
import LungoKit
import Polyglot
import XCTest

/// The scaler facility: multiplies by ten.
final class TenfoldScaler: PolyglotScaler {
    func hostScale(_ n: LungoNat) throws -> LungoNat {
        LungoNat(n.uint64! * 10)
    }
}

/// The journal facility: records lines, refusing `fail`.
final class RecordingJournal: PolyglotJournal {
    private let lock = NSLock()
    private(set) var lines: [String] = []

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

let journal: RecordingJournal = {
    let j = RecordingJournal()
    setScaler(TenfoldScaler())
    setJournal(j)
    return j
}()

/// Opens once `release` is called; a task waiting for it is cancelled with the task.
actor Gate {
    private var open = false

    func release() { open = true }

    func wait() async throws {
        while !open { try await Task.sleep(nanoseconds: 1_000_000) }
    }
}

struct NetworkDown: Error {}

/// Answers the program's operations: `get` waits for `gate` when the url is "slow", and throws
/// for "broken".
struct Fetcher: PolyglotFetchOpHandler {
    var gate: Gate? = nil

    func get(_ url: String) async throws -> LungoExcept<String, String> {
        switch url {
        case "slow": try await gate!.wait()
        case "broken": throw NetworkDown()
        case "missing": return .error("not found")
        default: break
        }
        return .ok("body:\(url)")
    }

    func stamp(_ t: Token) async throws -> Token { t }

    func scaler(_ k: LungoNat) async throws -> (LungoNat) throws -> LungoNat {
        { x in LungoNat(x.uint64! * k.uint64!) }
    }
}

/// TEST0297: run alone (`swift test --filter MissingFacilityTests`, with
/// LUNGO_POLYGLOT_WITHOUT_FACILITIES set), before any facility is installed.
final class MissingFacilityTests: XCTestCase {
    // TEST0297: a call before every facility is installed fails, naming the facility and an
    // operation of it
    func test0297_AMissingFacilityIsNamed() throws {
        try XCTSkipUnless(ProcessInfo.processInfo.environment["LUNGO_POLYGLOT_WITHOUT_FACILITIES"] != nil)
        XCTAssertThrowsError(try factorial(3)) { e in
            let missing = e as? LungoMissingFacility
            XCTAssertEqual(missing?.facility, "polyglot.journal")
            XCTAssertEqual(missing?.operation, "hostRecord")
        }
    }
}

final class PolyglotTests: XCTestCase {
    override func setUp() {
        _ = journal
    }

    // TEST0048: numbers
    func test0048_Numbers() throws {
        XCTAssertEqual(try factorial(25).description, "15511210043330985984000000")
        let big = LungoInt("-123456789012345678901234567890")!
        XCTAssertEqual(try negate(big).description, "123456789012345678901234567890")
    }

    // TEST0049: structures And Inductives
    func test0049_StructuresAndInductives() throws {
        let p = Point(x: 1.5, y: -2, label: "p", tag: 7)
        let moved = try moveBy(p, 1, 0.5)
        XCTAssertEqual(moved, Point(x: 2.5, y: -1.5, label: "p", tag: 7))
        XCTAssertEqual(try describe(moved), "p#7 at (2.500000, -1.500000)")
        XCTAssertEqual(try area(.rect(corner: moved, width: 3, height: 4)), 12)
        XCTAssertEqual(try area(.circle(center: p, radius: 2)), 12)
        XCTAssertEqual(try area(.empty), 0)
    }

    /// TEST0050: `Lookup` the type and `Registry.lookup` the function: `Lookup` and `lookup`.
    func test0050_AFunctionNamedLikeItsType() throws {
        XCTAssertEqual(try lookup(3, 1), Lookup.found(slot: 1))
        XCTAssertEqual(try lookup(3, 5), Lookup.missing)
    }

    // TEST0051: scalars And Containers
    func test0051_ScalarsAndContainers() throws {
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

    // TEST0052: errors
    func test0052_Errors() throws {
        XCTAssertThrowsError(try checkedDiv(10, 0)) { e in
            XCTAssertEqual((e as? LungoIOError)?.message, "checkedDiv: division by zero")
        }
        XCTAssertThrowsError(try parseDigit("x")) { e in
            XCTAssertEqual((e as? LungoError<String>)?.value, "not a digit: x")
        }
        XCTAssertEqual(try parseDigit("7"), 7)
    }

    // TEST0053: polymorphism
    func test0053_Polymorphism() throws {
        let tree = try ofList(Lungo.nat, [1, 2, 3])
        XCTAssertEqual(try size(Lungo.nat, tree), 3)
        let mirrored = try mirror(Lungo.nat, tree)
        guard case .node = mirrored else { return XCTFail("a mirrored tree is a node") }
        XCTAssertEqual(try toList(Lungo.nat, mirrored), [3, 2, 1])
        let strings: Tree<String> = .node(left: .leaf, value: "s", right: .leaf)
        XCTAssertEqual(try size(Lungo.string, strings), 1)
        XCTAssertEqual(tree, try ofList(Lungo.nat, [1, 2, 3]))
    }

    // TEST0054: functions
    func test0054_Functions() throws {
        var calls: [LungoNat] = []
        let twice = try applyTwice({ x in
            calls.append(x)
            return LungoNat(x.uint64! * 2 + 1)
        }, 3)
        XCTAssertEqual(twice, 15)
        XCTAssertEqual(calls, [3, 7])
        XCTAssertEqual(try adder(5, 3), 8)
    }

    // TEST0055: opaque Values
    func test0055_OpaqueValues() throws {
        let counter = try newCounter(10)
        XCTAssertEqual(try bump(counter), 11)
        XCTAssertEqual(try bump(counter), 12)
        counter.close()
        XCTAssertThrowsError(try bump(counter)) { XCTAssert($0 is LungoMalformed) }
    }

    // TEST0056: host Facilities
    func test0056_HostFacilities() throws {
        XCTAssertEqual(try scaledSum([1, 2, 3]), 60)
        journal.clear()
        XCTAssertEqual(try recordAll(["a", "b"]), 2)
        XCTAssertThrowsError(try recordAll(["c", "fail", "d"])) { e in
            XCTAssert((e as? LungoIOError)?.message.contains("refuses to record") == true)
        }
        XCTAssertEqual(journal.lines, ["a", "b", "c"])
    }

    // TEST0057: concurrent Calls
    func test0057_ConcurrentCalls() throws {
        let results = UnsafeMutableBufferPointer<UInt64>.allocate(capacity: 16)
        defer { results.deallocate() }
        DispatchQueue.concurrentPerform(iterations: 16) { i in
            results[i] = (try? factorial(LungoNat(UInt64(i))))?.uint64 ?? 0
        }
        XCTAssertEqual(results[10], 3_628_800)
        XCTAssert(results.allSatisfy { $0 > 0 })
    }

    // TEST0058: run Main
    func test0058_RunMain() throws {
        XCTAssertEqual(try runMain(["one", "two"]), 2)
    }

    // TEST0298: the package's assurance document is the program's: its claims, what they assume,
    // and the facilities each export needs
    func test0298_AssuranceIsTheProgramsAssurance() throws {
        let a = assurance
        XCTAssertEqual(a.program, "polyglot")
        XCTAssertEqual(a.schemaVersion, 1)
        XCTAssertEqual(a.provenance.leanVersion, "4.34.1")
        XCTAssertEqual(a.claim("Polyglot.factorial_pos")?.status, "proved")
        XCTAssertEqual(a.claim("Polyglot.factorial_pos")?.relation, "lungo.law")
        XCTAssertEqual(a.claim("Polyglot.factorial_pos")?.assumptions, [])
        XCTAssertEqual(a.claim("Polyglot.Tree.mirror_mirror")?.relation, "lungo.roundtrip")
        XCTAssertEqual(a.claim("Polyglot.divide_ok")?.specifications, ["Polyglot.natDiv"])
        // Proved, and conditional on what the host's scaler is assumed to do.
        XCTAssertEqual(a.claim("Polyglot.scaledSum_singleton_mono")?.status, "proved")
        XCTAssertEqual(a.claim("Polyglot.scaledSum_singleton_mono")?.assumptions, ["Polyglot.ScalesMonotonically"])
        XCTAssertEqual(a.export("Polyglot.scaledSum")?.facilities, ["Polyglot.Scaler"])
        XCTAssertEqual(a.export("Polyglot.scaledSum")?.assumptions, ["Polyglot.ScalesMonotonically"])
        XCTAssertEqual(a.export("Polyglot.fetchAll")?.isAsync, true)
        XCTAssertEqual(a.export("Polyglot.fetchAll")?.facilities, ["Polyglot.fetchInterface"])
        // Sorted by Lean name.
        XCTAssertEqual(
            a.facilities.map { "\($0.id)/\($0.form)" },
            ["polyglot.journal/extern", "polyglot.scaler/extern", "polyglot.fetch/async"]
        )
        XCTAssertEqual(a.export("Polyglot.negate")?.claims, [])
        XCTAssertEqual(try LungoAssurance.decode(assuranceJSON), a)
    }

    // TEST0299: an async program runs on its handler's answers: data, an opaque handle passed
    // back, and a host function the program calls
    func test0299_AnAsyncProgramRunsOnTheHandlersAnswers() async throws {
        let h = Fetcher()
        let bodies = try await fetchAll(h, ["a", "missing", "b"])
        XCTAssertEqual(bodies, ["body:a", "error: not found", "body:b"])
        let token = try XCTUnwrap(try mkToken(5))
        XCTAssertEqual(try await stampToken(h, token), 6)
        XCTAssertEqual(try await applyScaler(h, 7, 6), 42)
        XCTAssertEqual(lungoOutstanding(), 0)
    }

    // TEST0300: an error of the handler abandons the program: the call throws it, and nothing
    // waits
    func test0300_AHandlersErrorAbandonsTheProgram() async throws {
        do {
            _ = try await fetchAll(Fetcher(), ["a", "broken", "b"])
            XCTFail("the fetch succeeded")
        } catch {
            XCTAssert(error is NetworkDown, "\(error)")
        }
        XCTAssertEqual(lungoOutstanding(), 0)
    }

    // TEST0301: cancelling the task abandons a program waiting for an answer
    func test0301_CancellationAbandonsTheProgram() async throws {
        let task = Task { try await fetchAll(Fetcher(gate: Gate()), ["slow"]) }
        while lungoOutstanding() == 0 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(lungoOutstanding(), 1)
        task.cancel()
        do {
            _ = try await task.value
            XCTFail("the cancelled fetch succeeded")
        } catch {
            XCTAssert(error is CancellationError, "\(error)")
        }
        XCTAssertEqual(lungoOutstanding(), 0)
    }

    // TEST0302: many async programs waiting at once resume independently, answered in any order
    func test0302_ConcurrentAsyncProgramsResumeIndependently() async throws {
        let gate = Gate()
        let h = Fetcher(gate: gate)
        try await withThrowingTaskGroup(of: (Int, [String]).self) { group in
            for i in 0..<100 {
                group.addTask {
                    try await Task.sleep(nanoseconds: UInt64.random(in: 0..<20_000_000))
                    return (i, try await fetchAll(h, ["slow", String(repeating: "x", count: i)]))
                }
            }
            while lungoOutstanding() < 100 { try await Task.sleep(nanoseconds: 1_000_000) }
            await gate.release()
            for try await (i, got) in group {
                XCTAssertEqual(got, ["body:slow", "body:" + String(repeating: "x", count: i)])
            }
        }
        XCTAssertEqual(lungoOutstanding(), 0)
    }

    // TEST0303: the process ends while a program waits for an answer (this test runs last)
    func test0303_AProcessEndsWithAProgramWaiting() async throws {
        Task.detached { _ = try? await fetchAll(Fetcher(gate: Gate()), ["slow"]) }
        while lungoOutstanding() == 0 { try await Task.sleep(nanoseconds: 1_000_000) }
        XCTAssertEqual(lungoOutstanding(), 1)
    }
}
