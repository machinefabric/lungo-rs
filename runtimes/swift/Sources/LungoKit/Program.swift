import Foundation
import LungoRuntime

/// A handle to a Lean value Swift does not represent: the value lives until the handle is
/// closed or deinitialized.
final class LungoHandle: @unchecked Sendable {
    private let lock = NSLock()
    private var id: UInt64

    init(_ id: UInt64) { self.id = id }

    deinit { close() }

    func close() {
        lock.lock()
        defer { lock.unlock() }
        if id != 0 {
            lungo_handle_release(id)
            id = 0
        }
    }

    func live() throws -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        guard id != 0 else { throw LungoMalformed("the Lean value was closed") }
        return id
    }

    /// A new handle identifier of the value, owned by the caller.
    func clone() throws -> UInt64 { lungo_handle_clone(try live()) }
}

/// A Lean value of a type Swift does not represent, held by handle.
public final class LungoOpaque: @unchecked Sendable {
    let handle: LungoHandle

    init(_ handle: LungoHandle) { self.handle = handle }

    /// Releases the value; using it afterwards fails with `LungoMalformed`.
    public func close() { handle.close() }
}

/// A call of a generated entry point (`<prefix>call_<function>`) with its input: its status and
/// output. Generated packages make it with their own `lungo_buffer`.
public typealias LungoCall = (UnsafePointer<UInt8>?, Int) -> (Int32, [UInt8])

/// Copies the bytes of a runtime buffer and frees it.
func take(_ buf: inout lungo_buffer) -> [UInt8] {
    let out = buf.len > 0 ? Array(UnsafeBufferPointer(start: buf.data, count: Int(buf.len))) : []
    lungo_buffer_free(&buf)
    return out
}

/// What a function returns: a value, an `IO` result, or an `EIO ε` result.
public struct LungoReturns<R> {
    let read: (inout LungoReader) throws -> R
    let write: (inout LungoWriter, Result<R, Error>) throws -> Void

    /// A value; a host function's error is fatal to Lean.
    public static func value(_ t: LungoType<R>) -> LungoReturns<R> {
        LungoReturns(
            read: { try t.decode(&$0) },
            write: { w, result in
                switch result {
                case .success(let v): try t.encode(&w, v)
                case .failure(let e): throw LungoHostError("\(e)")
                }
            })
    }

    /// `IO α`: the value or a `LungoIOError`.
    public static func io(_ t: LungoType<R>) -> LungoReturns<R> {
        LungoReturns(
            read: { r in
                switch try r.u8() {
                case 0: return try t.decode(&r)
                case 1:
                    let handle = LungoHandle(try r.u64())
                    throw LungoIOError(message: try r.text(), handle: handle)
                case let tag: throw LungoMalformed("invalid IO result tag \(tag)")
                }
            },
            write: { w, result in
                switch result {
                case .success(let v):
                    w.u8(0)
                    try t.encode(&w, v)
                case .failure(let e as LungoIOError):
                    w.u8(1)
                    w.u64(try e.handle?.clone() ?? 0)
                    try w.blob(Array(e.message.utf8))
                case .failure(let e):
                    // Any other error of an IO extern is IO.userError with its description.
                    w.u8(1)
                    w.u64(0)
                    try w.blob(Array("\(e)".utf8))
                }
            })
    }

    /// `EIO ε α`: the value or a `LungoError<E>`.
    public static func eio<E>(_ e: LungoType<E>, _ t: LungoType<R>) -> LungoReturns<R> {
        LungoReturns(
            read: { r in
                switch try r.u8() {
                case 0: return try t.decode(&r)
                case 1: throw LungoError(try e.decode(&r))
                case let tag: throw LungoMalformed("invalid EIO result tag \(tag)")
                }
            },
            write: { w, result in
                switch result {
                case .success(let v):
                    w.u8(0)
                    try t.encode(&w, v)
                case .failure(let err as LungoError<E>):
                    w.u8(1)
                    try e.encode(&w, err.value)
                case .failure(let err):
                    throw LungoHostError("an EIO extern failed without an error value: \(err)")
                }
            })
    }
}

/// A generated program: its type table. Generated packages create one.
public final class LungoProgram: @unchecked Sendable {
    let types: OpaquePointer?

    public init(types: OpaquePointer?) {
        Host.install()
        self.types = types
    }

    /// Calls `entry` with type arguments and arguments (`args` encodes them).
    public func invoke<R>(
        _ call: LungoCall, typeArgs: [[UInt8]] = [], args: (inout LungoWriter) throws -> Void,
        returns: LungoReturns<R>
    ) throws -> R {
        var w = LungoWriter(program: self, result: false)
        defer { w.temps.forEach(Host.release) }
        w.u32(UInt32(typeArgs.count))
        typeArgs.forEach { w.raw($0) }
        try args(&w)
        let (status, out) = w.bytes.withUnsafeBufferPointer { call($0.baseAddress, $0.count) }
        return try result(status, out, returns.read)
    }

    func result<R>(_ status: Int32, _ out: [UInt8], _ read: (inout LungoReader) throws -> R) throws -> R {
        switch status {
        case 0: break
        case 1: throw LungoMalformed(String(decoding: out, as: UTF8.self))
        default: fatalError("lungo: a generated entry point returned status \(status)")
        }
        var r = LungoReader(out, program: self)
        let v: R
        do {
            v = try read(&r)
            try r.finish()
        } catch let e as LungoMalformed {
            fatalError("lungo: the runtime produced a malformed result: \(e.message)")
        }
        return v
    }

    func callClosure<R>(
        _ handle: LungoHandle, expr: [UInt8], args: (inout LungoWriter) throws -> Void, result read: LungoType<R>
    ) throws -> R {
        var w = LungoWriter(program: self, result: false)
        defer { w.temps.forEach(Host.release) }
        w.u32(0)
        try args(&w)
        let id = try handle.live()
        var buf = lungo_buffer()
        let status = w.bytes.withUnsafeBufferPointer { input in
            expr.withUnsafeBufferPointer { e in
                lungo_closure_call(types, id, e.baseAddress, e.count, input.baseAddress, input.count, &buf)
            }
        }
        let out = take(&buf)
        withExtendedLifetime(handle) {}
        return try result(status, out) { try read.decode(&$0) }
    }

    /// Implements host extern `index` with `call` (decoding its arguments and producing its
    /// result), through the program's `set_host_extern`.
    public func hostExtern<R>(
        _ setHostExtern: @convention(c) (Int, UInt64) -> Void, index: Int, returns: LungoReturns<R>,
        _ call: @escaping (inout LungoReader) throws -> R
    ) {
        let id = Host.register(self) { r, w in
            let result: Result<R, Error>
            do {
                result = .success(try call(&r))
            } catch let e as LungoMalformed {
                throw e
            } catch {
                result = .failure(error)
            }
            try returns.write(&w, result)
        }
        setHostExtern(index, id)
    }

    /// Runs the program's `main` (`runMain`) with `args`; its exit code.
    public func runMain(_ runMain: @convention(c) (Int, UnsafePointer<UnsafePointer<CChar>?>?) -> Int32, _ args: [String])
        throws -> Int32
    {
        for (i, a) in args.enumerated() where a.utf8.contains(0) {
            throw LungoMalformed("argument \(i) contains NUL")
        }
        var cstrings = args.map { strdup($0) }
        defer { cstrings.forEach { free($0) } }
        return cstrings.withUnsafeMutableBufferPointer { p in
            p.baseAddress!.withMemoryRebound(to: UnsafePointer<CChar>?.self, capacity: max(p.count, 1)) {
                runMain(args.count, $0)
            }
        }
    }
}

/// The host: Swift functions the runtime calls.
enum Host {
    typealias Call = (inout LungoReader, inout LungoWriter) throws -> Void

    private static let lock = NSLock()
    nonisolated(unsafe) private static var next: UInt64 = 0
    nonisolated(unsafe) private static var entries: [UInt64: (program: LungoProgram, call: Call, refs: Int)] = [:]

    private static let installed: Void = {
        lungo_set_host(dispatch, { retain($0) }, { release($0) })
    }()

    static func install() { _ = installed }

    /// Registers `call` with one reference, owned by the caller.
    static func register(_ program: LungoProgram, _ call: @escaping Call) -> UInt64 {
        lock.lock()
        defer { lock.unlock() }
        next += 1
        entries[next] = (program, call, 1)
        return next
    }

    static func retain(_ id: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        guard entries[id] != nil else { fatalError("lungo: the runtime retained host function \(id), which is not registered") }
        entries[id]!.refs += 1
    }

    static func release(_ id: UInt64) {
        lock.lock()
        defer { lock.unlock() }
        guard let e = entries[id] else { fatalError("lungo: host function \(id) is released more often than it is referenced") }
        if e.refs == 1 { entries[id] = nil } else { entries[id]!.refs -= 1 }
    }

    static func entry(_ id: UInt64) -> (program: LungoProgram, call: Call) {
        lock.lock()
        defer { lock.unlock() }
        guard let e = entries[id] else { fatalError("lungo: the runtime called host function \(id), which is not registered") }
        return (e.program, e.call)
    }
}

private let dispatch: lungo_host_dispatch = { callback, input, len, out in
    let (program, call) = Host.entry(callback)
    var r = LungoReader(len > 0 ? Array(UnsafeBufferPointer(start: input, count: Int(len))) : [], program: program)
    var w = LungoWriter(program: program, result: true)
    var status: Int32 = 0
    var payload: [UInt8]
    do {
        try call(&r, &w)
        try r.finish()
        payload = w.bytes
    } catch {
        status = 1
        payload = Array("\(error)".utf8)
    }
    if !payload.isEmpty, let dst = lungo_buffer_alloc(out, payload.count) {
        payload.withUnsafeBufferPointer { dst.update(from: $0.baseAddress!, count: $0.count) }
    }
    return status
}

extension LungoWriter {
    /// Registers `call` for this writer's call: lent for the call when the values are
    /// arguments, given to the runtime when they are a result.
    mutating func hostFunction(_ call: @escaping Host.Call) throws {
        guard let program else { throw LungoMalformed("a function outside a call of a Lean program") }
        let id = Host.register(program, call)
        if !result { temps.append(id) }
        u8(1)
        u64(id)
    }
}

extension LungoReader {
    /// A function value the runtime sent: a Lean closure by handle.
    mutating func leanFunction() throws -> (LungoProgram, LungoHandle) {
        let kind = try u8()
        let id = try u64()
        guard kind == 0 else { throw LungoMalformed("the runtime sent a function of kind \(kind)") }
        guard let program else { throw LungoMalformed("a function outside a call of a Lean program") }
        return (program, LungoHandle(id))
    }
}
