// The polyglot assertions, in Go, on the generated Go package.
package polyglottest

import (
	"errors"
	"math/big"
	"strings"
	"sync"
	"testing"

	lungo "github.com/machinefabric/lungo-go"

	"example.com/polyglottest/polyglot"
)

type host struct {
	mu    sync.Mutex
	lines []string
}

func (h *host) HostScale(n *big.Int) (*big.Int, error) {
	return new(big.Int).Mul(n, big.NewInt(10)), nil
}

func (h *host) HostRecord(line string) error {
	if line == "fail" {
		return lungo.NewIOError("the host refuses to record `fail`")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.lines = append(h.lines, line)
	return nil
}

var theHost = &host{}

func init() { polyglot.SetHost(theHost) }

// must(f(…))(t) is the result of a call that must succeed.
func must[T any](v T, err error) func(t *testing.T) T {
	return func(t *testing.T) T {
		t.Helper()
		if err != nil {
			t.Fatalf("unexpected error: %v", err)
		}
		return v
	}
}

func TestNumbers(t *testing.T) {
	f := must(polyglot.Factorial(big.NewInt(25)))(t)
	if f.String() != "15511210043330985984000000" {
		t.Errorf("25! = %s", f)
	}
	large, _ := new(big.Int).SetString("-123456789012345678901234567890", 10)
	if n := must(polyglot.Negate(large))(t); n.String() != "123456789012345678901234567890" {
		t.Errorf("negate = %s", n)
	}
	var m *lungo.MalformedError
	if _, err := polyglot.Factorial(new(big.Int).SetInt64(-1)); !errors.As(err, &m) {
		t.Errorf("a negative Nat was not rejected: %v", err)
	}
}

func TestStructuresAndInductives(t *testing.T) {
	p := polyglot.Point{X: 1.5, Y: -2, Label: "p", Tag: 7}
	moved := must(polyglot.MoveBy(p, 1, 0.5))(t)
	if moved != (polyglot.Point{X: 2.5, Y: -1.5, Label: "p", Tag: 7}) {
		t.Errorf("moveBy = %+v", moved)
	}
	if d := must(polyglot.Describe(moved))(t); d != "p#7 at (2.500000, -1.500000)" {
		t.Errorf("describe = %q", d)
	}
	if a := must(polyglot.Area(polyglot.ShapeRect{Corner: moved, Width: 3, Height: 4}))(t); a != 12 {
		t.Errorf("area of rect = %v", a)
	}
	if a := must(polyglot.Area(&polyglot.ShapeCircle{Center: p, Radius: 2}))(t); a != 12 {
		t.Errorf("area of circle = %v", a)
	}
	if a := must(polyglot.Area(polyglot.ShapeEmpty{}))(t); a != 0 {
		t.Errorf("area of empty = %v", a)
	}
	var m *lungo.MalformedError
	if _, err := polyglot.Area(nil); !errors.As(err, &m) {
		t.Errorf("a nil Shape was not rejected: %v", err)
	}
}

func TestScalarsAndContainers(t *testing.T) {
	if s := must(polyglot.Mix(^uint64(0), -5, 'λ', true))(t); s != "18446744073709551615/-5/λ/true" {
		t.Errorf("mix = %q", s)
	}
	if b := must(polyglot.ReverseBytes([]byte{1, 2, 3}))(t); string(b) != "\x03\x02\x01" {
		t.Errorf("reverseBytes = %v", b)
	}
	if s := must(polyglot.SumFloats([]float64{0.5, 0.25, 2}))(t); s != 2.75 {
		t.Errorf("sumFloats = %v", s)
	}
	if w := must(polyglot.FirstWord("hello world"))(t); !w.Valid || w.Value != "hello" {
		t.Errorf("firstWord = %+v", w)
	}
	if w := must(polyglot.FirstWord(""))(t); w.Valid {
		t.Errorf("firstWord of empty = %+v", w)
	}
	if p := must(polyglot.Swap(lungo.Pair[string, *big.Int]{First: "x", Second: big.NewInt(9)}))(t); p.First.Int64() != 9 || p.Second != "x" {
		t.Errorf("swap = %+v", p)
	}
	evens := must(polyglot.Evens(big.NewInt(10)))(t)
	if len(evens) != 5 || evens[4].Int64() != 8 {
		t.Errorf("evens = %v", evens)
	}
	if q := must(polyglot.Divide(big.NewInt(10), big.NewInt(0)))(t); q.Ok || q.Error != "division by zero" {
		t.Errorf("divide = %+v", q)
	}
	var m *lungo.MalformedError
	if _, err := polyglot.FirstWord("\xff"); !errors.As(err, &m) {
		t.Errorf("invalid UTF-8 was not rejected: %v", err)
	}
}

func TestErrors(t *testing.T) {
	_, err := polyglot.CheckedDiv(big.NewInt(10), big.NewInt(0))
	var ioe *lungo.IOError
	if !errors.As(err, &ioe) || ioe.Error() != "checkedDiv: division by zero" {
		t.Errorf("checkedDiv = %v", err)
	}
	_, err = polyglot.ParseDigit('x')
	var le *lungo.Error[string]
	if !errors.As(err, &le) || le.Value != "not a digit: x" {
		t.Errorf("parseDigit = %v", err)
	}
	if d := must(polyglot.ParseDigit('7'))(t); d.Int64() != 7 {
		t.Errorf("parseDigit 7 = %v", d)
	}
}

func TestPolymorphism(t *testing.T) {
	tree := must(polyglot.OfList(lungo.NatType, []*big.Int{big.NewInt(1), big.NewInt(2), big.NewInt(3)}))(t)
	if n := must(polyglot.Size(lungo.NatType, tree))(t); n.Int64() != 3 {
		t.Errorf("size = %v", n)
	}
	back := must(polyglot.ToList(lungo.NatType, must(polyglot.Mirror(lungo.NatType, tree))(t)))(t)
	if len(back) != 3 || back[0].Int64() != 3 {
		t.Errorf("mirrored = %v", back)
	}
	if _, ok := must(polyglot.Mirror(lungo.NatType, tree))(t).(polyglot.TreeNode[*big.Int]); !ok {
		t.Error("a mirrored tree is a node")
	}
	strs := polyglot.TreeNode[string]{Left: polyglot.TreeLeaf[string]{}, Value: "s", Right: polyglot.TreeLeaf[string]{}}
	if n := must(polyglot.Size(lungo.StringType, polyglot.Tree[string](strs)))(t); n.Int64() != 1 {
		t.Errorf("size of a string tree = %v", n)
	}
}

func TestFunctions(t *testing.T) {
	calls := 0
	twice := must(polyglot.ApplyTwice(func(x *big.Int) (*big.Int, error) {
		calls++
		return new(big.Int).Add(new(big.Int).Mul(x, big.NewInt(2)), big.NewInt(1)), nil
	}, big.NewInt(3)))(t)
	if twice.Int64() != 15 || calls != 2 {
		t.Errorf("applyTwice = %v after %d calls", twice, calls)
	}
	if v := must(polyglot.Adder(big.NewInt(5), big.NewInt(3)))(t); v.Int64() != 8 {
		t.Errorf("adder = %v", v)
	}
}

func TestOpaqueValues(t *testing.T) {
	counter := must(polyglot.NewCounter(big.NewInt(10)))(t)
	v1 := must(polyglot.Bump(counter))(t)
	v2 := must(polyglot.Bump(counter))(t)
	if v1.Int64() != 11 || v2.Int64() != 12 {
		t.Errorf("bump = %v, %v", v1, v2)
	}
	counter.Close()
	var m *lungo.MalformedError
	if _, err := polyglot.Bump(counter); !errors.As(err, &m) {
		t.Errorf("a closed handle was not rejected: %v", err)
	}
}

func TestHostExterns(t *testing.T) {
	if s := must(polyglot.ScaledSum([]*big.Int{big.NewInt(1), big.NewInt(2), big.NewInt(3)}))(t); s.Int64() != 60 {
		t.Errorf("scaledSum = %v", s)
	}
	theHost.mu.Lock()
	theHost.lines = nil
	theHost.mu.Unlock()
	if n := must(polyglot.RecordAll([]string{"a", "b"}))(t); n.Int64() != 2 {
		t.Errorf("recordAll = %v", n)
	}
	_, err := polyglot.RecordAll([]string{"c", "fail", "d"})
	if err == nil || !strings.Contains(err.Error(), "refuses to record") {
		t.Errorf("recordAll with a failure = %v", err)
	}
	if strings.Join(theHost.lines, ",") != "a,b,c" {
		t.Errorf("recorded %v", theHost.lines)
	}
}

func TestConcurrentCalls(t *testing.T) {
	var wg sync.WaitGroup
	for i := 0; i < 16; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			f, err := polyglot.Factorial(big.NewInt(int64(i)))
			if err != nil || f.Sign() <= 0 {
				t.Errorf("factorial %d = %v, %v", i, f, err)
			}
		}(i)
	}
	wg.Wait()
}

func TestRunMain(t *testing.T) {
	if code := must(polyglot.RunMain([]string{"one", "two"}))(t); code != 2 {
		t.Errorf("main returned %d", code)
	}
}
