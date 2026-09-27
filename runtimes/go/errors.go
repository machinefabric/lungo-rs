package lungo

import (
	"fmt"
	"runtime"
)

// IOError is an IO.Error of Lean: raised by a Lean function, or by a host function (with
// NewIOError) to fail an IO extern.
type IOError struct {
	message string
	// handle is the Lean IO.Error, or 0 for one raised by Go.
	handle *Handle
}

// NewIOError is an IO.userError with message, for a host function to return.
func NewIOError(message string) *IOError { return &IOError{message: message} }

func (e *IOError) Error() string { return e.message }

// Error is the error value of an `EIO ε` function.
type Error[E any] struct {
	Value E
}

func (e *Error[E]) Error() string { return fmt.Sprintf("Lean error: %v", e.Value) }

// MalformedError reports arguments the program rejected: values Go can express that Lean
// cannot (a string that is not UTF-8, a negative Nat, a nil value of an inductive type).
type MalformedError struct {
	Message string
}

func (e *MalformedError) Error() string { return "lungo: malformed arguments: " + e.Message }

// HostError reports a host function that failed where Lean cannot observe the failure.
type HostError struct {
	Message string
}

func (e *HostError) Error() string { return e.Message }

func malformed(format string, args ...any) error {
	return &MalformedError{Message: fmt.Sprintf(format, args...)}
}

// Malformed is a MalformedError with a formatted message. Generated packages use it.
func Malformed(format string, args ...any) error { return malformed(format, args...) }

// keepAlive keeps v reachable until this point.
func keepAlive(v any) { runtime.KeepAlive(v) }
