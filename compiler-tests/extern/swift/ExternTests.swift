import Consumer
import LungoKit
import Provider
import XCTest

/// A value made by one program is used by the other: both run on the one runtime.
final class ExternTests: XCTestCase {
    // TEST0005: values Pass Between Programs
    func test0005_ValuesPassBetweenPrograms() throws {
        let p = try XCTUnwrap(try Provider.mkPos(3))
        let d = try Consumer.double(p)
        XCTAssertEqual(try Provider.value(d), 6)
        let q = try Provider.makePair(1, "apples")
        XCTAssertEqual(try Consumer.describe(try Consumer.count(d, q)), "apples: 7")
    }
}
