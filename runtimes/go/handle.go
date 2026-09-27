package lungo

/*
#include "lungo.h"
*/
import "C"

import (
	"runtime"
	"sync"
)

// Handle holds a Lean value Go cannot represent (an opaque value, a Lean closure, an
// IO.Error). The value lives while the Handle is reachable, or until Close.
type Handle struct {
	mu sync.Mutex
	id uint64
}

func newHandle(id uint64) *Handle {
	h := &Handle{id: id}
	runtime.SetFinalizer(h, (*Handle).Close)
	return h
}

// Close releases the Lean value; using the Handle afterwards is an error.
func (h *Handle) Close() {
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.id != 0 {
		C.lungo_handle_release(C.uint64_t(h.id))
		h.id = 0
	}
}

// live is the handle's identifier, or an error if it was closed.
func (h *Handle) live() (uint64, error) {
	if h == nil {
		return 0, malformed("a nil handle")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	if h.id == 0 {
		return 0, malformed("a closed handle")
	}
	return h.id, nil
}

// clone is a new identifier of the handle's value, which the caller owns.
func (h *Handle) clone() (uint64, error) {
	id, err := h.live()
	if err != nil {
		return 0, err
	}
	return uint64(C.lungo_handle_clone(C.uint64_t(id))), nil
}

// Opaque is a Lean value of a type Go does not represent, held by handle.
type Opaque struct {
	h *Handle
}

// Close releases the value.
func (o Opaque) Close() { o.h.Close() }
