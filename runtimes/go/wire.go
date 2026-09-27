package lungo

import (
	"encoding/binary"
	"math"
	"unicode/utf8"
	"unsafe"
)

// Program is a generated program, as the wire format needs it: its type table.
type Program struct {
	types unsafe.Pointer
}

// NewProgram is the program with the type table `types` (a `const lungo_types *`). Generated
// packages call it.
func NewProgram(types unsafe.Pointer) *Program { return &Program{types: types} }

// Writer encodes values in the wire format.
type Writer struct {
	program *Program
	buf     []byte
	// result: the values are a result (their handles and host functions are given to the
	// runtime) rather than arguments (lent for the call).
	result bool
	// temps: host functions registered for the call, released when it returns.
	temps []uint64
}

// NewCall starts encoding a call to a function of p with the type arguments `types` (in
// order): the input of a generated entry point.
func NewCall(p *Program, types ...interface{ Expr() []byte }) *Writer {
	w := &Writer{program: p}
	w.U32(uint32(len(types)))
	for _, t := range types {
		w.buf = append(w.buf, t.Expr()...)
	}
	return w
}

// Bytes is the encoding.
func (w *Writer) Bytes() []byte { return w.buf }

// Release ends the call: the runtime no longer needs the host functions it was lent.
func (w *Writer) Release() {
	for _, id := range w.temps {
		hostRelease(id)
	}
	w.temps = nil
}

func (w *Writer) U8(v uint8)    { w.buf = append(w.buf, v) }
func (w *Writer) U16(v uint16)  { w.buf = binary.LittleEndian.AppendUint16(w.buf, v) }
func (w *Writer) U32(v uint32)  { w.buf = binary.LittleEndian.AppendUint32(w.buf, v) }
func (w *Writer) U64(v uint64)  { w.buf = binary.LittleEndian.AppendUint64(w.buf, v) }
func (w *Writer) F64(v float64) { w.U64(math.Float64bits(v)) }
func (w *Writer) F32(v float32) { w.U32(math.Float32bits(v)) }

// Len writes a length or count, which the wire format limits to 32 bits.
func (w *Writer) Len(n int) error {
	if uint64(n) > math.MaxUint32 {
		return malformed("a length of %d exceeds the wire format's limit", n)
	}
	w.U32(uint32(n))
	return nil
}

// Blob writes length-prefixed bytes.
func (w *Writer) Blob(b []byte) error {
	if err := w.Len(len(b)); err != nil {
		return err
	}
	w.buf = append(w.buf, b...)
	return nil
}

// Reader decodes values in the wire format, as the runtime produced them.
type Reader struct {
	program *Program
	data    []byte
	pos     int
}

// NewReader decodes data of program p.
func NewReader(p *Program, data []byte) *Reader { return &Reader{program: p, data: data} }

func (r *Reader) take(n int) ([]byte, error) {
	if n < 0 || n > len(r.data)-r.pos {
		return nil, malformed("wire data ends after %d bytes; %d more expected", len(r.data), n)
	}
	b := r.data[r.pos : r.pos+n]
	r.pos += n
	return b, nil
}

func (r *Reader) U8() (uint8, error) {
	b, err := r.take(1)
	if err != nil {
		return 0, err
	}
	return b[0], nil
}

func (r *Reader) U16() (uint16, error) {
	b, err := r.take(2)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint16(b), nil
}

func (r *Reader) U32() (uint32, error) {
	b, err := r.take(4)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint32(b), nil
}

func (r *Reader) U64() (uint64, error) {
	b, err := r.take(8)
	if err != nil {
		return 0, err
	}
	return binary.LittleEndian.Uint64(b), nil
}

func (r *Reader) F64() (float64, error) {
	v, err := r.U64()
	return math.Float64frombits(v), err
}

func (r *Reader) F32() (float32, error) {
	v, err := r.U32()
	return math.Float32frombits(v), err
}

// Count reads a length or count of items of at least `unit` bytes, which must fit the rest.
func (r *Reader) Count(unit int) (int, error) {
	n, err := r.U32()
	if err != nil {
		return 0, err
	}
	if unit < 1 {
		unit = 1
	}
	if uint64(n)*uint64(unit) > uint64(len(r.data)-r.pos) {
		return 0, malformed("a length of %d exceeds the remaining wire data", n)
	}
	return int(n), nil
}

// Blob reads length-prefixed bytes (a copy).
func (r *Reader) Blob() ([]byte, error) {
	n, err := r.Count(1)
	if err != nil {
		return nil, err
	}
	b, err := r.take(n)
	if err != nil {
		return nil, err
	}
	return append([]byte(nil), b...), nil
}

// Text reads a UTF-8 string.
func (r *Reader) Text() (string, error) {
	b, err := r.Blob()
	if err != nil {
		return "", err
	}
	if !utf8.Valid(b) {
		return "", malformed("a string is not valid UTF-8")
	}
	return string(b), nil
}

// Finish fails unless every byte was read.
func (r *Reader) Finish() error {
	if r.pos != len(r.data) {
		return malformed("%d unexpected bytes after the wire data", len(r.data)-r.pos)
	}
	return nil
}
