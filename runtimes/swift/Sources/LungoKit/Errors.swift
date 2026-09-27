/// An `IO.Error` of Lean: thrown by a Lean function, or by a host function
/// (`LungoIOError(message)`) to fail an `IO` extern with `IO.userError`.
public struct LungoIOError: Error, CustomStringConvertible, @unchecked Sendable {
    public let message: String
    let handle: LungoHandle?

    public init(_ message: String) {
        self.message = message
        self.handle = nil
    }

    init(message: String, handle: LungoHandle) {
        self.message = message
        self.handle = handle
    }

    public var description: String { message }
}

/// The error value of an `EIO ε` function; a host function of an `EIO ε` extern throws it with
/// its error value.
public struct LungoError<E>: Error, @unchecked Sendable {
    public let value: E

    public init(_ value: E) { self.value = value }
}

/// Arguments Lean cannot represent (a negative number for `Nat`, a closed handle).
public struct LungoMalformed: Error, CustomStringConvertible, Sendable {
    public let message: String

    public init(_ message: String) { self.message = message }

    public var description: String { "malformed arguments: \(message)" }
}

/// A host function failed where Lean cannot observe the failure.
public struct LungoHostError: Error, CustomStringConvertible, Sendable {
    public let message: String

    public init(_ message: String) { self.message = message }

    public var description: String { message }
}
