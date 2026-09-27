package lungo

import (
	"encoding/binary"
	"math/big"
	"unicode/utf8"
)

// Tags of type expressions.
const (
	tagNat        = 0
	tagInt        = 1
	tagBool       = 2
	tagUInt8      = 3
	tagUInt16     = 4
	tagUInt32     = 5
	tagUInt64     = 6
	tagUSize      = 7
	tagInt8       = 8
	tagInt16      = 9
	tagInt32      = 10
	tagInt64      = 11
	tagISize      = 12
	tagFloat      = 13
	tagFloat32    = 14
	tagChar       = 15
	tagString     = 16
	tagUnit       = 17
	tagByteArray  = 18
	tagFloatArray = 19
	tagOption     = 20
	tagList       = 21
	tagArray      = 22
	tagProd       = 23
	tagExcept     = 24
	tagFunction   = 25
	tagInductive  = 27
	tagOpaque     = 28
)

// Type describes how values of the Go type T cross the boundary as a Lean type: its type
// expression and its wire encoding. Polymorphic functions take one per type parameter.
type Type[T any] interface {
	// Expr is the encoded type expression.
	Expr() []byte
	Encode(w *Writer, v T) error
	Decode(r *Reader) (T, error)
}

// Unit is Lean's Unit.
type Unit struct{}

// Option is Lean's Option: Value when Valid.
type Option[T any] struct {
	Value T
	Valid bool
}

func Some[T any](v T) Option[T] { return Option[T]{Value: v, Valid: true} }
func None[T any]() Option[T]    { return Option[T]{} }

// Pair is Lean's Prod.
type Pair[A, B any] struct {
	First  A
	Second B
}

// Except is Lean's Except: Value when Ok, else Error.
type Except[E, A any] struct {
	Error E
	Value A
	Ok    bool
}

func Ok[E, A any](v A) Except[E, A]  { return Except[E, A]{Value: v, Ok: true} }
func Err[E, A any](e E) Except[E, A] { return Except[E, A]{Error: e} }

type simple[T any] struct {
	tag    byte
	encode func(w *Writer, v T) error
	decode func(r *Reader) (T, error)
}

func (s simple[T]) Expr() []byte                { return []byte{s.tag} }
func (s simple[T]) Encode(w *Writer, v T) error { return s.encode(w, v) }
func (s simple[T]) Decode(r *Reader) (T, error) { return s.decode(r) }

func putMagnitude(w *Writer, m *big.Int) error {
	be := m.Bytes()
	le := make([]byte, len(be))
	for i, b := range be {
		le[len(be)-1-i] = b
	}
	return w.Blob(le)
}

func magnitude(r *Reader) (*big.Int, error) {
	le, err := r.Blob()
	if err != nil {
		return nil, err
	}
	if len(le) > 0 && le[len(le)-1] == 0 {
		return nil, malformed("a number's magnitude has a leading zero byte")
	}
	be := make([]byte, len(le))
	for i, b := range le {
		be[len(le)-1-i] = b
	}
	return new(big.Int).SetBytes(be), nil
}

// NatType is Lean's Nat: a non-negative *big.Int.
var NatType Type[*big.Int] = simple[*big.Int]{tagNat,
	func(w *Writer, v *big.Int) error {
		if v == nil || v.Sign() < 0 {
			return malformed("a Nat is nil or negative")
		}
		return putMagnitude(w, v)
	},
	magnitude,
}

// IntType is Lean's Int: a *big.Int.
var IntType Type[*big.Int] = simple[*big.Int]{tagInt,
	func(w *Writer, v *big.Int) error {
		if v == nil {
			return malformed("an Int is nil")
		}
		if v.Sign() < 0 {
			w.U8(1)
		} else {
			w.U8(0)
		}
		return putMagnitude(w, new(big.Int).Abs(v))
	},
	func(r *Reader) (*big.Int, error) {
		sign, err := r.U8()
		if err != nil {
			return nil, err
		}
		m, err := magnitude(r)
		if err != nil {
			return nil, err
		}
		switch {
		case sign == 0:
			return m, nil
		case sign == 1 && m.Sign() == 0:
			return nil, malformed("negative zero is not a canonical Int")
		case sign == 1:
			return m.Neg(m), nil
		}
		return nil, malformed("invalid Int sign %d", sign)
	},
}

var BoolType Type[bool] = simple[bool]{tagBool,
	func(w *Writer, v bool) error {
		if v {
			w.U8(1)
		} else {
			w.U8(0)
		}
		return nil
	},
	func(r *Reader) (bool, error) {
		b, err := r.U8()
		if err != nil {
			return false, err
		}
		if b > 1 {
			return false, malformed("invalid Bool %d", b)
		}
		return b == 1, nil
	},
}

var UInt8Type Type[uint8] = simple[uint8]{tagUInt8, func(w *Writer, v uint8) error { w.U8(v); return nil }, (*Reader).U8}
var UInt16Type Type[uint16] = simple[uint16]{tagUInt16, func(w *Writer, v uint16) error { w.U16(v); return nil }, (*Reader).U16}
var UInt32Type Type[uint32] = simple[uint32]{tagUInt32, func(w *Writer, v uint32) error { w.U32(v); return nil }, (*Reader).U32}
var UInt64Type Type[uint64] = simple[uint64]{tagUInt64, func(w *Writer, v uint64) error { w.U64(v); return nil }, (*Reader).U64}

// USizeType is Lean's USize, as a uint64 (the runtime rejects values its platform's USize
// cannot hold).
var USizeType Type[uint64] = simple[uint64]{tagUSize, func(w *Writer, v uint64) error { w.U64(v); return nil }, (*Reader).U64}

var Int8Type Type[int8] = simple[int8]{tagInt8,
	func(w *Writer, v int8) error { w.U8(uint8(v)); return nil },
	func(r *Reader) (int8, error) { v, err := r.U8(); return int8(v), err },
}
var Int16Type Type[int16] = simple[int16]{tagInt16,
	func(w *Writer, v int16) error { w.U16(uint16(v)); return nil },
	func(r *Reader) (int16, error) { v, err := r.U16(); return int16(v), err },
}
var Int32Type Type[int32] = simple[int32]{tagInt32,
	func(w *Writer, v int32) error { w.U32(uint32(v)); return nil },
	func(r *Reader) (int32, error) { v, err := r.U32(); return int32(v), err },
}
var Int64Type Type[int64] = simple[int64]{tagInt64,
	func(w *Writer, v int64) error { w.U64(uint64(v)); return nil },
	func(r *Reader) (int64, error) { v, err := r.U64(); return int64(v), err },
}

// ISizeType is Lean's ISize, as an int64.
var ISizeType Type[int64] = simple[int64]{tagISize,
	func(w *Writer, v int64) error { w.U64(uint64(v)); return nil },
	func(r *Reader) (int64, error) { v, err := r.U64(); return int64(v), err },
}

var FloatType Type[float64] = simple[float64]{tagFloat, func(w *Writer, v float64) error { w.F64(v); return nil }, (*Reader).F64}
var Float32Type Type[float32] = simple[float32]{tagFloat32, func(w *Writer, v float32) error { w.F32(v); return nil }, (*Reader).F32}

// CharType is Lean's Char: a rune that is a Unicode scalar value.
var CharType Type[rune] = simple[rune]{tagChar,
	func(w *Writer, v rune) error {
		if !utf8.ValidRune(v) {
			return malformed("%#x is not a Unicode scalar value", v)
		}
		w.U32(uint32(v))
		return nil
	},
	func(r *Reader) (rune, error) {
		v, err := r.U32()
		if err != nil {
			return 0, err
		}
		if !utf8.ValidRune(rune(v)) || v > utf8.MaxRune {
			return 0, malformed("%#x is not a Unicode scalar value", v)
		}
		return rune(v), nil
	},
}

// StringType is Lean's String: a valid UTF-8 string.
var StringType Type[string] = simple[string]{tagString,
	func(w *Writer, v string) error {
		if !utf8.ValidString(v) {
			return malformed("a string is not valid UTF-8")
		}
		return w.Blob([]byte(v))
	},
	(*Reader).Text,
}

var UnitType Type[Unit] = simple[Unit]{tagUnit,
	func(*Writer, Unit) error { return nil },
	func(*Reader) (Unit, error) { return Unit{}, nil },
}

var ByteArrayType Type[[]byte] = simple[[]byte]{tagByteArray, (*Writer).Blob, (*Reader).Blob}

var FloatArrayType Type[[]float64] = simple[[]float64]{tagFloatArray,
	func(w *Writer, v []float64) error {
		if err := w.Len(len(v)); err != nil {
			return err
		}
		for _, x := range v {
			w.F64(x)
		}
		return nil
	},
	func(r *Reader) ([]float64, error) {
		n, err := r.Count(8)
		if err != nil {
			return nil, err
		}
		out := make([]float64, n)
		for i := range out {
			if out[i], err = r.F64(); err != nil {
				return nil, err
			}
		}
		return out, nil
	},
}

// OpaqueType is a Lean value Go does not represent, held by handle.
var OpaqueType Type[Opaque] = simple[Opaque]{tagOpaque,
	func(w *Writer, v Opaque) error {
		id, err := w.handle(v.h)
		if err != nil {
			return err
		}
		w.U64(id)
		return nil
	},
	func(r *Reader) (Opaque, error) {
		id, err := r.U64()
		if err != nil {
			return Opaque{}, err
		}
		return Opaque{h: newHandle(id)}, nil
	},
}

// handle writes the identifier of h: lent for arguments, a clone given away for results.
func (w *Writer) handle(h *Handle) (uint64, error) {
	if w.result {
		return h.clone()
	}
	return h.live()
}

func expr(tag byte, parts ...[]byte) []byte {
	out := []byte{tag}
	for _, p := range parts {
		out = append(out, p...)
	}
	return out
}

type optionType[T any] struct{ t Type[T] }

// OptionType is Lean's `Option α`.
func OptionType[T any](t Type[T]) Type[Option[T]] { return optionType[T]{t} }

func (o optionType[T]) Expr() []byte { return expr(tagOption, o.t.Expr()) }
func (o optionType[T]) Encode(w *Writer, v Option[T]) error {
	if !v.Valid {
		w.U8(0)
		return nil
	}
	w.U8(1)
	return o.t.Encode(w, v.Value)
}
func (o optionType[T]) Decode(r *Reader) (Option[T], error) {
	tag, err := r.U8()
	if err != nil {
		return Option[T]{}, err
	}
	switch tag {
	case 0:
		return Option[T]{}, nil
	case 1:
		v, err := o.t.Decode(r)
		return Option[T]{Value: v, Valid: err == nil}, err
	}
	return Option[T]{}, malformed("invalid Option tag %d", tag)
}

type seqType[T any] struct {
	tag byte
	t   Type[T]
}

// ListType is Lean's `List α`, as a slice.
func ListType[T any](t Type[T]) Type[[]T] { return seqType[T]{tagList, t} }

// ArrayType is Lean's `Array α`, as a slice.
func ArrayType[T any](t Type[T]) Type[[]T] { return seqType[T]{tagArray, t} }

func (s seqType[T]) Expr() []byte { return expr(s.tag, s.t.Expr()) }
func (s seqType[T]) Encode(w *Writer, v []T) error {
	if err := w.Len(len(v)); err != nil {
		return err
	}
	for _, x := range v {
		if err := s.t.Encode(w, x); err != nil {
			return err
		}
	}
	return nil
}
func (s seqType[T]) Decode(r *Reader) ([]T, error) {
	n, err := r.Count(1)
	if err != nil {
		return nil, err
	}
	out := make([]T, n)
	for i := range out {
		if out[i], err = s.t.Decode(r); err != nil {
			return nil, err
		}
	}
	return out, nil
}

type pairType[A, B any] struct {
	a Type[A]
	b Type[B]
}

// PairType is Lean's `α × β`.
func PairType[A, B any](a Type[A], b Type[B]) Type[Pair[A, B]] { return pairType[A, B]{a, b} }

func (p pairType[A, B]) Expr() []byte { return expr(tagProd, p.a.Expr(), p.b.Expr()) }
func (p pairType[A, B]) Encode(w *Writer, v Pair[A, B]) error {
	if err := p.a.Encode(w, v.First); err != nil {
		return err
	}
	return p.b.Encode(w, v.Second)
}
func (p pairType[A, B]) Decode(r *Reader) (Pair[A, B], error) {
	a, err := p.a.Decode(r)
	if err != nil {
		return Pair[A, B]{}, err
	}
	b, err := p.b.Decode(r)
	return Pair[A, B]{a, b}, err
}

type exceptType[E, A any] struct {
	e Type[E]
	a Type[A]
}

// ExceptType is Lean's `Except ε α`.
func ExceptType[E, A any](e Type[E], a Type[A]) Type[Except[E, A]] { return exceptType[E, A]{e, a} }

func (x exceptType[E, A]) Expr() []byte { return expr(tagExcept, x.e.Expr(), x.a.Expr()) }
func (x exceptType[E, A]) Encode(w *Writer, v Except[E, A]) error {
	if v.Ok {
		w.U8(1)
		return x.a.Encode(w, v.Value)
	}
	w.U8(0)
	return x.e.Encode(w, v.Error)
}
func (x exceptType[E, A]) Decode(r *Reader) (Except[E, A], error) {
	tag, err := r.U8()
	if err != nil {
		return Except[E, A]{}, err
	}
	switch tag {
	case 0:
		e, err := x.e.Decode(r)
		return Except[E, A]{Error: e}, err
	case 1:
		a, err := x.a.Decode(r)
		return Except[E, A]{Value: a, Ok: true}, err
	}
	return Except[E, A]{}, malformed("invalid Except tag %d", tag)
}

// InductiveExpr is the type expression of the type at `index` of a program's type table
// applied to `args`. Generated packages use it.
func InductiveExpr(index uint32, args ...[]byte) []byte {
	out := []byte{tagInductive}
	out = binary.LittleEndian.AppendUint32(out, index)
	out = binary.LittleEndian.AppendUint32(out, uint32(len(args)))
	for _, a := range args {
		out = append(out, a...)
	}
	return out
}
