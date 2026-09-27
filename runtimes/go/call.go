package lungo

import (
	"errors"
	"fmt"
	"strings"
)

// Status of a generated entry point.
const (
	statusOK        = 0
	statusMalformed = 1
)

// result decodes the output of a generated entry point with status `status`.
func result(p *Program, status int32, out []byte, decode func(r *Reader) error) error {
	switch status {
	case statusOK:
	case statusMalformed:
		return &MalformedError{Message: string(out)}
	default:
		panic(fmt.Sprintf("lungo: a generated entry point returned status %d", status))
	}
	r := NewReader(p, out)
	if err := decode(r); err != nil {
		panic("lungo: the runtime produced a malformed result: " + err.Error())
	}
	if err := r.Finish(); err != nil {
		panic("lungo: the runtime produced a malformed result: " + err.Error())
	}
	return nil
}

// DecodeValue is the result of a call returning a value of t.
func DecodeValue[T any](p *Program, status int32, out []byte, t Type[T]) (T, error) {
	var v T
	err := result(p, status, out, func(r *Reader) (err error) {
		v, err = t.Decode(r)
		return err
	})
	return v, err
}

// decodeIOError reads an IO.Error: a handle to it, and its message.
func decodeIOError(r *Reader) (*IOError, error) {
	id, err := r.U64()
	if err != nil {
		return nil, err
	}
	msg, err := r.Text()
	if err != nil {
		return nil, err
	}
	return &IOError{message: msg, handle: newHandle(id)}, nil
}

// DecodeIO is the result of a call of an `IO α` function: the value of t, or its *IOError.
func DecodeIO[T any](p *Program, status int32, out []byte, t Type[T]) (T, error) {
	var v T
	var failure error
	err := result(p, status, out, func(r *Reader) error {
		tag, err := r.U8()
		if err != nil {
			return err
		}
		switch tag {
		case 0:
			v, err = t.Decode(r)
			return err
		case 1:
			e, err := decodeIOError(r)
			failure = e
			return err
		}
		return malformed("invalid IO result tag %d", tag)
	})
	if err != nil {
		return v, err
	}
	return v, failure
}

// DecodeEIO is the result of a call of an `EIO ε α` function: the value of t, or its
// *Error[E].
func DecodeEIO[E, T any](p *Program, status int32, out []byte, e Type[E], t Type[T]) (T, error) {
	var v T
	var failure error
	err := result(p, status, out, func(r *Reader) error {
		tag, err := r.U8()
		if err != nil {
			return err
		}
		switch tag {
		case 0:
			v, err = t.Decode(r)
			return err
		case 1:
			ev, err := e.Decode(r)
			failure = &Error[E]{Value: ev}
			return err
		}
		return malformed("invalid EIO result tag %d", tag)
	})
	if err != nil {
		return v, err
	}
	return v, failure
}

// WriteValue writes the result of a host extern returning a value of t. The extern cannot
// fail: an error terminates the Lean program with its message.
func WriteValue[T any](w *Writer, t Type[T], v T, err error) error {
	if err != nil {
		return &HostError{Message: err.Error()}
	}
	return t.Encode(w, v)
}

// WriteIO writes the result of a host extern of type `IO α`: the value, or the error as an
// IO.Error (an *IOError received from Lean is raised again; any other error becomes
// IO.userError with its message).
func WriteIO[T any](w *Writer, t Type[T], v T, err error) error {
	if err == nil {
		w.U8(0)
		return t.Encode(w, v)
	}
	w.U8(1)
	var ioe *IOError
	if errors.As(err, &ioe) && ioe.handle != nil {
		id, cerr := ioe.handle.clone()
		if cerr != nil {
			return cerr
		}
		w.U64(id)
		return w.Blob([]byte(ioe.message))
	}
	w.U64(0)
	return w.Blob([]byte(strings.ToValidUTF8(err.Error(), "\uFFFD")))
}

// WriteEIO writes the result of a host extern of type `EIO ε α`: the value, or the error
// value of an *Error[E]. Any other error terminates the Lean program with its message.
func WriteEIO[E, T any](w *Writer, e Type[E], t Type[T], v T, err error) error {
	if err == nil {
		w.U8(0)
		return t.Encode(w, v)
	}
	var lean *Error[E]
	if !errors.As(err, &lean) {
		return &HostError{Message: "a host extern of an EIO type failed without an error value: " + err.Error()}
	}
	w.U8(1)
	return e.Encode(w, lean.Value)
}
