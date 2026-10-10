// The ledger example from Swift: the proved reducer, and the claims the package carries.
import Ledger
import LungoKit
import XCTest

final class LedgerTests: XCTestCase {
    // TEST0327: the proved ledger from Swift
    func test0327_TheProvedLedgerFromSwift() throws {
        XCTAssertEqual(try replay([.deposit(amount: 100), .withdraw(amount: 30), .deposit(amount: 5)]), 75)
        XCTAssertNil(try replay([.withdraw(amount: 1), .deposit(amount: 100)]), "an overdraft is refused")
        let claim = try XCTUnwrap(assurance.claim("Ledger.replay_solvent"))
        XCTAssertEqual(claim.status, "proved")
        XCTAssertEqual(claim.specifications, ["Ledger.Solvent"])
    }
}
