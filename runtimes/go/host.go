package lungo

/*
#include "lungo.h"
*/
import "C"

import (
	"fmt"
	"sync"
	"unsafe"
)

// A Go function the runtime can call: it decodes its arguments from r and encodes its result
// into w.
type hostEntry struct {
	program *Program
	call    func(r *Reader, w *Writer) error
	refs    int
}

var registry = struct {
	sync.Mutex
	next    uint64
	entries map[uint64]*hostEntry
}{entries: map[uint64]*hostEntry{}}

// hostRegister registers call with one reference, owned by the caller.
func hostRegister(p *Program, call func(r *Reader, w *Writer) error) uint64 {
	registry.Lock()
	defer registry.Unlock()
	registry.next++
	id := registry.next
	registry.entries[id] = &hostEntry{program: p, call: call, refs: 1}
	return id
}

func hostEntryOf(id uint64) *hostEntry {
	registry.Lock()
	defer registry.Unlock()
	e, ok := registry.entries[id]
	if !ok {
		panic(fmt.Sprintf("lungo: the runtime referenced host function %d, which is not registered", id))
	}
	return e
}

func hostRetain(id uint64) {
	registry.Lock()
	defer registry.Unlock()
	e, ok := registry.entries[id]
	if !ok {
		panic(fmt.Sprintf("lungo: the runtime retained host function %d, which is not registered", id))
	}
	e.refs++
}

func hostRelease(id uint64) {
	registry.Lock()
	defer registry.Unlock()
	e, ok := registry.entries[id]
	if !ok {
		panic(fmt.Sprintf("lungo: host function %d is released more often than it is referenced", id))
	}
	e.refs--
	if e.refs == 0 {
		delete(registry.entries, id)
	}
}

// hostFunction encodes the Go function call: lent for the call when the values are
// arguments, given to the runtime when they are a result.
func (w *Writer) hostFunction(call func(r *Reader, w *Writer) error) {
	id := hostRegister(w.program, call)
	if !w.result {
		w.temps = append(w.temps, id)
	}
	w.U8(1)
	w.U64(id)
}

// RegisterExtern registers the implementation of a host extern of program p for the life of
// the process; the identifier is for the program's `set_host_extern`. Generated packages use
// it.
func RegisterExtern(p *Program, call func(r *Reader, w *Writer) error) uint64 {
	return hostRegister(p, call)
}

//export lungoGoDispatch
func lungoGoDispatch(callback C.uint64_t, input *C.uint8_t, n C.size_t, out *C.lungo_buffer) C.int32_t {
	e := hostEntryOf(uint64(callback))
	var in []byte
	if n > 0 {
		in = C.GoBytes(unsafe.Pointer(input), C.int(n))
	}
	r := NewReader(e.program, in)
	w := &Writer{program: e.program, result: true}
	err := e.call(r, w)
	if err == nil {
		err = r.Finish()
	}
	bytes := w.buf
	status := C.int32_t(0)
	if err != nil {
		bytes = []byte(err.Error())
		status = 1
	}
	if len(bytes) > 0 {
		dst := C.lungo_buffer_alloc(out, C.size_t(len(bytes)))
		copy(unsafe.Slice((*byte)(unsafe.Pointer(dst)), len(bytes)), bytes)
	}
	return status
}

//export lungoGoRetain
func lungoGoRetain(callback C.uint64_t) { hostRetain(uint64(callback)) }

//export lungoGoRelease
func lungoGoRelease(callback C.uint64_t) { hostRelease(uint64(callback)) }

// callClosure calls the Lean closure h of type expression `expr` of program p: args encodes
// the arguments, result decodes the result.
func callClosure(p *Program, h *Handle, expr []byte, args func(w *Writer) error, result func(r *Reader) error) error {
	w := NewCall(p)
	defer w.Release()
	if err := args(w); err != nil {
		return err
	}
	id, err := h.live()
	if err != nil {
		return err
	}
	var buf C.lungo_buffer
	status := C.lungo_closure_call((*C.lungo_types)(p.types), C.uint64_t(id), bytesPtr(expr), C.size_t(len(expr)),
		bytesPtr(w.buf), C.size_t(len(w.buf)), &buf)
	keepAlive(h)
	bytes := takeBuffer(&buf)
	if status != 0 {
		return &MalformedError{Message: string(bytes)}
	}
	r := NewReader(p, bytes)
	if err := result(r); err != nil {
		panic("lungo: the runtime produced a malformed result: " + err.Error())
	}
	if err := r.Finish(); err != nil {
		panic("lungo: the runtime produced a malformed result: " + err.Error())
	}
	return nil
}

func bytesPtr(b []byte) *C.uint8_t {
	if len(b) == 0 {
		return nil
	}
	return (*C.uint8_t)(unsafe.Pointer(&b[0]))
}

// takeBuffer copies the bytes of a runtime buffer and frees it.
func takeBuffer(buf *C.lungo_buffer) []byte {
	var out []byte
	if buf.len > 0 {
		out = C.GoBytes(unsafe.Pointer(buf.data), C.int(buf.len))
	}
	C.lungo_buffer_free(buf)
	return out
}

// leanFunction reads a function value from the runtime: a Lean closure by handle.
func (r *Reader) leanFunction() (*Handle, error) {
	kind, err := r.U8()
	if err != nil {
		return nil, err
	}
	id, err := r.U64()
	if err != nil {
		return nil, err
	}
	if kind != 0 {
		return nil, malformed("the runtime sent a function of kind %d", kind)
	}
	return newHandle(id), nil
}
