package lungo

import (
	"encoding/hex"
	"encoding/json"
	"fmt"
	"math"
	"math/big"
	"os"
	"strconv"
	"testing"
)

// The shared wire-format vectors (compiler-tests/wire/vectors.json), which every language's
// support library must encode and decode exactly as the runtime does.
type vectors struct {
	Valid []struct {
		Name, Type, Bytes string
		Value             any
	}
	Invalid []struct{ Name, Type, Bytes string }
}

func loadVectors(t *testing.T) vectors {
	data, err := os.ReadFile("testdata/vectors.json")
	if err != nil {
		t.Fatalf("the wire vectors are missing: %v", err)
	}
	var v vectors
	if err := json.Unmarshal(data, &v); err != nil {
		t.Fatal(err)
	}
	if len(v.Valid) == 0 || len(v.Invalid) == 0 {
		t.Fatal("the wire vectors are empty")
	}
	return v
}

// dyn is a Type of JSON-represented values built from the library's typed descriptors, so the
// vectors exercise them.
type dyn struct {
	expr   []byte
	encode func(w *Writer, v any) error
	decode func(r *Reader) (any, error)
}

func (d dyn) Expr() []byte                  { return d.expr }
func (d dyn) Encode(w *Writer, v any) error { return d.encode(w, v) }
func (d dyn) Decode(r *Reader) (any, error) { return d.decode(r) }

// lift adapts a typed descriptor to JSON values with conversions to and from JSON.
func lift[T any](t Type[T], from func(any) (T, error), to func(T) any) dyn {
	return dyn{
		expr: t.Expr(),
		encode: func(w *Writer, v any) error {
			x, err := from(v)
			if err != nil {
				return err
			}
			return t.Encode(w, x)
		},
		decode: func(r *Reader) (any, error) {
			x, err := t.Decode(r)
			if err != nil {
				return nil, err
			}
			return to(x), nil
		},
	}
}

func bigOf(v any) (*big.Int, error) {
	s, ok := v.(string)
	n, parsed := new(big.Int).SetString(s, 10)
	if !ok || !parsed {
		return nil, fmt.Errorf("not a decimal string: %v", v)
	}
	return n, nil
}

func numberOf(v any) float64 { return v.(float64) }

func bitsOf(v any) uint64 {
	n, err := strconv.ParseUint(v.(map[string]any)["bits"].(string), 16, 64)
	if err != nil {
		panic(err)
	}
	return n
}

func toBits(b uint64, width int) any { return map[string]any{"bits": fmt.Sprintf("%0*x", width, b)} }

func typeOf(t *testing.T, r *Reader) dyn {
	tag, err := r.U8()
	if err != nil {
		t.Fatal(err)
	}
	str := func(b *big.Int) any { return b.String() }
	switch tag {
	case tagNat:
		return lift(NatType, bigOf, str)
	case tagInt:
		return lift(IntType, bigOf, str)
	case tagBool:
		return lift(BoolType, func(v any) (bool, error) { return v.(bool), nil }, func(b bool) any { return b })
	case tagUInt8:
		return lift(UInt8Type, func(v any) (uint8, error) { return uint8(numberOf(v)), nil }, func(x uint8) any { return float64(x) })
	case tagUInt16:
		return lift(UInt16Type, func(v any) (uint16, error) { return uint16(numberOf(v)), nil }, func(x uint16) any { return float64(x) })
	case tagUInt32:
		return lift(UInt32Type, func(v any) (uint32, error) { return uint32(numberOf(v)), nil }, func(x uint32) any { return float64(x) })
	case tagUInt64, tagUSize:
		ty := UInt64Type
		if tag == tagUSize {
			ty = USizeType
		}
		return lift(ty, func(v any) (uint64, error) { return strconv.ParseUint(v.(string), 10, 64) },
			func(x uint64) any { return strconv.FormatUint(x, 10) })
	case tagInt8:
		return lift(Int8Type, func(v any) (int8, error) { return int8(numberOf(v)), nil }, func(x int8) any { return float64(x) })
	case tagInt16:
		return lift(Int16Type, func(v any) (int16, error) { return int16(numberOf(v)), nil }, func(x int16) any { return float64(x) })
	case tagInt32:
		return lift(Int32Type, func(v any) (int32, error) { return int32(numberOf(v)), nil }, func(x int32) any { return float64(x) })
	case tagInt64, tagISize:
		ty := Int64Type
		if tag == tagISize {
			ty = ISizeType
		}
		return lift(ty, func(v any) (int64, error) { return strconv.ParseInt(v.(string), 10, 64) },
			func(x int64) any { return strconv.FormatInt(x, 10) })
	case tagFloat:
		return lift(FloatType, func(v any) (float64, error) { return math.Float64frombits(bitsOf(v)), nil },
			func(x float64) any { return toBits(math.Float64bits(x), 16) })
	case tagFloat32:
		return lift(Float32Type, func(v any) (float32, error) { return math.Float32frombits(uint32(bitsOf(v))), nil },
			func(x float32) any { return toBits(uint64(math.Float32bits(x)), 8) })
	case tagChar:
		return lift(CharType, func(v any) (rune, error) { return []rune(v.(string))[0], nil }, func(c rune) any { return string(c) })
	case tagString:
		return lift(StringType, func(v any) (string, error) { return v.(string), nil }, func(s string) any { return s })
	case tagUnit:
		return lift(UnitType, func(any) (Unit, error) { return Unit{}, nil }, func(Unit) any { return nil })
	case tagByteArray:
		return lift(ByteArrayType, func(v any) ([]byte, error) { return hex.DecodeString(v.(map[string]any)["bytes"].(string)) },
			func(b []byte) any { return map[string]any{"bytes": hex.EncodeToString(b)} })
	case tagFloatArray:
		return lift(FloatArrayType, func(v any) ([]float64, error) {
			var out []float64
			for _, x := range v.([]any) {
				n, err := strconv.ParseUint(x.(string), 16, 64)
				if err != nil {
					return nil, err
				}
				out = append(out, math.Float64frombits(n))
			}
			return out, nil
		}, func(xs []float64) any {
			out := []any{}
			for _, x := range xs {
				out = append(out, fmt.Sprintf("%016x", math.Float64bits(x)))
			}
			return out
		})
	case tagOption:
		inner := typeOf(t, r)
		return lift(OptionType[any](inner), func(v any) (Option[any], error) {
			m := v.(map[string]any)
			if x, ok := m["some"]; ok {
				return Some(x), nil
			}
			return None[any](), nil
		}, func(o Option[any]) any {
			if o.Valid {
				return map[string]any{"some": o.Value}
			}
			return map[string]any{"none": nil}
		})
	case tagList, tagArray:
		inner := typeOf(t, r)
		ty := ListType[any](inner)
		if tag == tagArray {
			ty = ArrayType[any](inner)
		}
		return lift(ty, func(v any) ([]any, error) { return v.([]any), nil }, func(xs []any) any {
			if xs == nil {
				return []any{}
			}
			return xs
		})
	case tagProd:
		a, b := typeOf(t, r), typeOf(t, r)
		return lift(PairType[any, any](a, b), func(v any) (Pair[any, any], error) {
			xs := v.([]any)
			return Pair[any, any]{xs[0], xs[1]}, nil
		}, func(p Pair[any, any]) any { return []any{p.First, p.Second} })
	case tagExcept:
		e, a := typeOf(t, r), typeOf(t, r)
		return lift(ExceptType[any, any](e, a), func(v any) (Except[any, any], error) {
			m := v.(map[string]any)
			if x, ok := m["ok"]; ok {
				return Ok[any, any](x), nil
			}
			return Err[any, any](m["error"]), nil
		}, func(x Except[any, any]) any {
			if x.Ok {
				return map[string]any{"ok": x.Value}
			}
			return map[string]any{"error": x.Error}
		})
	case tagFunction:
		n, err := r.U32()
		if err != nil {
			t.Fatal(err)
		}
		for i := uint32(0); i <= n; i++ {
			typeOf(t, r)
		}
		// Function values are handles and host callbacks, which name live objects of a
		// running program: only their rejection is checked here.
		return lift(Func1Type[any, any](dyn{}, dyn{}), nil, nil)
	case tagOpaque:
		return dyn{expr: []byte{tagOpaque}}
	}
	t.Fatalf("unknown type tag %d", tag)
	return dyn{}
}

func parseType(t *testing.T, h string) (dyn, byte) {
	b, err := hex.DecodeString(h)
	if err != nil {
		t.Fatal(err)
	}
	r := NewReader(nil, b)
	d := typeOf(t, r)
	if err := r.Finish(); err != nil {
		t.Fatal(err)
	}
	return d, b[0]
}

func TestValidVectorsRoundTrip(t *testing.T) {
	for _, v := range loadVectors(t).Valid {
		ty, tag := parseType(t, v.Type)
		if tag == tagFunction || tag == tagOpaque {
			continue
		}
		want, _ := hex.DecodeString(v.Bytes)
		w := &Writer{}
		if err := ty.Encode(w, v.Value); err != nil {
			t.Errorf("%s: encoding failed: %v", v.Name, err)
			continue
		}
		if hex.EncodeToString(w.Bytes()) != v.Bytes {
			t.Errorf("%s: encoded %x, want %s", v.Name, w.Bytes(), v.Bytes)
		}
		r := NewReader(nil, want)
		got, err := ty.Decode(r)
		if err == nil {
			err = r.Finish()
		}
		if err != nil {
			t.Errorf("%s: decoding failed: %v", v.Name, err)
			continue
		}
		gotJSON, _ := json.Marshal(got)
		wantJSON, _ := json.Marshal(v.Value)
		if string(gotJSON) != string(wantJSON) {
			t.Errorf("%s: decoded %s, want %s", v.Name, gotJSON, wantJSON)
		}
	}
}

func TestInvalidVectorsAreRejected(t *testing.T) {
	for _, v := range loadVectors(t).Invalid {
		ty, _ := parseType(t, v.Type)
		data, _ := hex.DecodeString(v.Bytes)
		r := NewReader(nil, data)
		_, err := ty.Decode(r)
		if err == nil {
			err = r.Finish()
		}
		if err == nil {
			t.Errorf("%s: %s was accepted", v.Name, v.Bytes)
		}
	}
}

func TestGoValuesLeanCannotRepresentAreRejected(t *testing.T) {
	w := &Writer{}
	for name, err := range map[string]error{
		"negative Nat":   NatType.Encode(w, big.NewInt(-1)),
		"nil Nat":        NatType.Encode(w, nil),
		"invalid UTF-8":  StringType.Encode(w, "\xff"),
		"surrogate Char": CharType.Encode(w, 0xd800),
		"nil function":   Func1Type(NatType, NatType).Encode(w, nil),
	} {
		var m *MalformedError
		if err == nil || !asMalformed(err, &m) {
			t.Errorf("%s: want a MalformedError, got %v", name, err)
		}
	}
}

func asMalformed(err error, m **MalformedError) bool {
	e, ok := err.(*MalformedError)
	*m = e
	return ok
}
