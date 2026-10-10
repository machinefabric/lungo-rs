// The polyglot assertions, in Go, on the generated Go package.
package polyglottest

import (
	"context"
	"errors"
	"math/big"
	"math/rand"
	"os"
	"os/exec"
	"strings"
	"sync"
	"testing"
	"time"

	lungo "github.com/machinefabric/lungo-go"

	"example.com/polyglottest/polyglot"
)

// The scaler capability: multiplies by ten.
type scaler struct{}

func (scaler) HostScale(n *big.Int) (*big.Int, error) {
	return new(big.Int).Mul(n, big.NewInt(10)), nil
}

// The journal capability: records lines, refusing `fail`.
type journal struct {
	mu    sync.Mutex
	lines []string
}

func (h *journal) HostRecord(line string) error {
	if line == "fail" {
		return lungo.NewIOError("the host refuses to record `fail`")
	}
	h.mu.Lock()
	defer h.mu.Unlock()
	h.lines = append(h.lines, line)
	return nil
}

var theJournal = &journal{}

// withoutCapabilities: the process runs a helper test that needs no capability installed.
const withoutCapabilities = "LUNGO_POLYGLOT_WITHOUT_CAPABILITIES"

func init() {
	if os.Getenv(withoutCapabilities) == "" {
		polyglot.SetScaler(scaler{})
		polyglot.SetJournal(theJournal)
	}
}

// helper runs the test `name` of this binary in a process of its own, with `env`.
func helper(t *testing.T, name string, env ...string) (string, error) {
	t.Helper()
	cmd := exec.Command(os.Args[0], "-test.run=^"+name+"$", "-test.count=1")
	cmd.Env = append(os.Environ(), env...)
	out, err := cmd.CombinedOutput()
	return string(out), err
}

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

// TEST0048: numbers
func Test0048_Numbers(t *testing.T) {
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

// TEST0049: structures And Inductives
func Test0049_StructuresAndInductives(t *testing.T) {
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

// TEST0050: The Lean type `Lookup` and the function `Registry.lookup` are both `Lookup` in Go's one
// case: the type keeps the name and the function is `RegistryLookup`.
func Test0050_AFunctionNamedLikeItsType(t *testing.T) {
	found := must(polyglot.RegistryLookup(big.NewInt(3), big.NewInt(1)))(t)
	if hit, ok := found.(polyglot.LookupFound); !ok || hit.Slot.Int64() != 1 {
		t.Errorf("lookup of a slot there is = %+v", found)
	}
	var answer polyglot.Lookup = must(polyglot.RegistryLookup(big.NewInt(3), big.NewInt(5)))(t)
	if _, ok := answer.(polyglot.LookupMissing); !ok {
		t.Errorf("lookup of a slot there is not = %+v", answer)
	}
}

// TEST0051: scalars And Containers
func Test0051_ScalarsAndContainers(t *testing.T) {
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

// TEST0052: errors
func Test0052_Errors(t *testing.T) {
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

// TEST0053: polymorphism
func Test0053_Polymorphism(t *testing.T) {
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

// TEST0054: functions
func Test0054_Functions(t *testing.T) {
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

// TEST0055: opaque Values
func Test0055_OpaqueValues(t *testing.T) {
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

// TEST0056: host capabilities
func Test0056_HostCapabilities(t *testing.T) {
	if s := must(polyglot.ScaledSum([]*big.Int{big.NewInt(1), big.NewInt(2), big.NewInt(3)}))(t); s.Int64() != 60 {
		t.Errorf("scaledSum = %v", s)
	}
	theJournal.mu.Lock()
	theJournal.lines = nil
	theJournal.mu.Unlock()
	if n := must(polyglot.RecordAll([]string{"a", "b"}))(t); n.Int64() != 2 {
		t.Errorf("recordAll = %v", n)
	}
	_, err := polyglot.RecordAll([]string{"c", "fail", "d"})
	if err == nil || !strings.Contains(err.Error(), "refuses to record") {
		t.Errorf("recordAll with a failure = %v", err)
	}
	if strings.Join(theJournal.lines, ",") != "a,b,c" {
		t.Errorf("recorded %v", theJournal.lines)
	}
}

// TEST0057: concurrent Calls
func Test0057_ConcurrentCalls(t *testing.T) {
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

// TEST0058: run Main
func Test0058_RunMain(t *testing.T) {
	if code := must(polyglot.RunMain([]string{"one", "two"}))(t); code != 2 {
		t.Errorf("main returned %d", code)
	}
}

// TEST0297: a call before every capability is installed fails, naming the capability and an
// operation of it.
func Test0297_AMissingCapabilityIsNamed(t *testing.T) {
	out, err := helper(t, "TestHelperWithoutCapabilities", withoutCapabilities+"=1")
	if err != nil {
		t.Fatalf("the helper failed: %v\n%s", err, out)
	}
	if !strings.Contains(out, "missing: polyglot.journal hostRecord") {
		t.Errorf("%s", out)
	}
}

func TestHelperWithoutCapabilities(t *testing.T) {
	if os.Getenv(withoutCapabilities) == "" {
		t.Skip("runs in a process of its own, from Test0297")
	}
	_, err := polyglot.Factorial(big.NewInt(3))
	var missing *lungo.MissingCapabilityError
	if !errors.As(err, &missing) {
		t.Fatalf("a call without capabilities returned %v", err)
	}
	// The journal is checked first: capabilities are checked in identifier order.
	t.Logf("missing: %s %s", missing.Capability, missing.Operation)
	os.Stdout.WriteString("missing: " + missing.Capability + " " + missing.Operation + "\n")
}

// TEST0298: the package's assurance document is the program's: its claims, what they assume, and
// the capabilities each export needs.
func Test0298_AssuranceIsTheProgramsAssurance(t *testing.T) {
	a := polyglot.Assurance()
	if a.Program != "polyglot" || a.SchemaVersion != 1 || a.Provenance.LeanVersion != "4.34.1" {
		t.Fatalf("assurance of %s, schema %d, Lean %s", a.Program, a.SchemaVersion, a.Provenance.LeanVersion)
	}
	claim := func(name string) *lungo.AssuranceClaim {
		c := a.Claim(name)
		if c == nil {
			t.Fatalf("no claim %s", name)
		}
		return c
	}
	if c := claim("Polyglot.factorial_pos"); c.Status != "proved" || c.Relation != "lungo.law" || len(c.Assumptions) != 0 {
		t.Errorf("factorial_pos: %+v", c)
	}
	if c := claim("Polyglot.Tree.mirror_mirror"); c.Relation != "lungo.roundtrip" {
		t.Errorf("mirror_mirror: %+v", c)
	}
	if c := claim("Polyglot.divide_ok"); c.Relation != "lungo.equals" || strings.Join(c.Specifications, ",") != "Polyglot.natDiv" {
		t.Errorf("divide_ok: %+v", c)
	}
	// Proved, and conditional on what the host's scaler is assumed to do.
	if c := claim("Polyglot.scaledSum_singleton_mono"); c.Status != "proved" || strings.Join(c.Assumptions, ",") != "Polyglot.ScalesMonotonically" {
		t.Errorf("scaledSum_singleton_mono: %+v", c)
	}
	e := a.Export("Polyglot.scaledSum")
	if e == nil || strings.Join(e.Capabilities, ",") != "Polyglot.Scaler" || strings.Join(e.Assumptions, ",") != "Polyglot.ScalesMonotonically" {
		t.Errorf("scaledSum: %+v", e)
	}
	if e := a.Export("Polyglot.fetchAll"); e == nil || !e.Async || strings.Join(e.Capabilities, ",") != "Polyglot.fetchInterface" {
		t.Errorf("fetchAll: %+v", e)
	}
	// Sorted by Lean name: Polyglot.Journal, Polyglot.Scaler, Polyglot.fetchInterface.
	ids := []string{}
	for _, c := range a.Capabilities {
		ids = append(ids, c.ID+"/"+c.Form)
	}
	if strings.Join(ids, ",") != "polyglot.journal/extern,polyglot.scaler/extern,polyglot.fetch/async" {
		t.Errorf("capabilities %v", ids)
	}
	if e := a.Export("Polyglot.negate"); e == nil || len(e.Claims) != 0 {
		t.Errorf("negate carries no claim: %+v", e)
	}
}

// fetcher answers the program's operations; get blocks until `release` is closed when the url is
// "slow", and fails with a Go error for "broken".
type fetcher struct {
	release chan struct{}
	k       int64
}

func (f *fetcher) Get(ctx context.Context, url string) (lungo.Except[string, string], error) {
	switch url {
	case "slow":
		select {
		case <-f.release:
		case <-ctx.Done():
			return lungo.Except[string, string]{}, ctx.Err()
		}
	case "broken":
		return lungo.Except[string, string]{}, errors.New("the network is down")
	case "missing":
		return lungo.Err[string, string]("not found"), nil
	}
	return lungo.Ok[string, string]("body:" + url), nil
}

func (f *fetcher) Stamp(ctx context.Context, token polyglot.Token) (polyglot.Token, error) {
	return token, nil
}

func (f *fetcher) Scaler(ctx context.Context, k *big.Int) (func(*big.Int) (*big.Int, error), error) {
	return func(x *big.Int) (*big.Int, error) { return new(big.Int).Mul(x, k), nil }, nil
}

// TEST0299: an async program runs on its handler's answers: data, an opaque handle passed back,
// and a host function the program calls.
func Test0299_AnAsyncProgramRunsOnTheHandlersAnswers(t *testing.T) {
	ctx := context.Background()
	h := &fetcher{}
	got := must(polyglot.FetchAll(ctx, h, []string{"a", "missing", "b"}))(t)
	if strings.Join(got, "|") != "body:a|error: not found|body:b" {
		t.Errorf("fetchAll = %v", got)
	}
	tok := must(polyglot.MkToken(big.NewInt(5)))(t)
	if !tok.Valid {
		t.Fatal("mkToken 5 is a token")
	}
	if n := must(polyglot.StampToken(ctx, h, tok.Value))(t); n.Int64() != 6 {
		t.Errorf("stampToken = %v", n)
	}
	if n := must(polyglot.ApplyScaler(ctx, h, big.NewInt(7), big.NewInt(6)))(t); n.Int64() != 42 {
		t.Errorf("applyScaler = %v", n)
	}
	if n := lungo.Outstanding(); n != 0 {
		t.Errorf("%d programs still wait", n)
	}
}

// TEST0300: an error of the handler abandons the program: the call returns it, and nothing waits.
func Test0300_AHandlersErrorAbandonsTheProgram(t *testing.T) {
	_, err := polyglot.FetchAll(context.Background(), &fetcher{}, []string{"a", "broken", "b"})
	if err == nil || err.Error() != "the network is down" {
		t.Errorf("fetchAll = %v", err)
	}
	if n := lungo.Outstanding(); n != 0 {
		t.Errorf("%d programs still wait", n)
	}
}

// TEST0301: cancelling the context abandons a program waiting for an answer.
func Test0301_CancellationAbandonsTheProgram(t *testing.T) {
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		_, err := polyglot.FetchAll(ctx, &fetcher{release: make(chan struct{})}, []string{"slow"})
		done <- err
	}()
	time.Sleep(50 * time.Millisecond)
	if n := lungo.Outstanding(); n != 1 {
		t.Errorf("%d programs wait for the slow fetch", n)
	}
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Errorf("a cancelled fetch returned %v", err)
	}
	if n := lungo.Outstanding(); n != 0 {
		t.Errorf("%d programs still wait", n)
	}
}

// TEST0302: many async programs waiting at once resume independently, answered in any order.
func Test0302_ConcurrentAsyncProgramsResumeIndependently(t *testing.T) {
	release := make(chan struct{})
	h := &fetcher{release: release}
	var wg sync.WaitGroup
	for i := 0; i < 100; i++ {
		wg.Add(1)
		go func(i int) {
			defer wg.Done()
			time.Sleep(time.Duration(rand.Intn(20)) * time.Millisecond)
			urls := []string{"slow", strings.Repeat("x", i)}
			got, err := polyglot.FetchAll(context.Background(), h, urls)
			if err != nil || len(got) != 2 || got[1] != "body:"+urls[1] {
				t.Errorf("program %d: %v, %v", i, got, err)
			}
		}(i)
	}
	time.Sleep(100 * time.Millisecond)
	close(release)
	wg.Wait()
	if n := lungo.Outstanding(); n != 0 {
		t.Errorf("%d programs still wait", n)
	}
}

// TEST0303: a process may end while a program waits for an answer.
func Test0303_AProcessEndsWithAProgramWaiting(t *testing.T) {
	out, err := helper(t, "TestHelperEndWhileWaiting", "LUNGO_POLYGLOT_END_WHILE_WAITING=1")
	if err != nil || !strings.Contains(out, "waiting: 1") {
		t.Errorf("the helper ended with %v:\n%s", err, out)
	}
}

func TestHelperEndWhileWaiting(t *testing.T) {
	if os.Getenv("LUNGO_POLYGLOT_END_WHILE_WAITING") == "" {
		t.Skip("runs in a process of its own, from Test0303")
	}
	go polyglot.FetchAll(context.Background(), &fetcher{release: make(chan struct{})}, []string{"slow"})
	for lungo.Outstanding() == 0 {
		time.Sleep(time.Millisecond)
	}
	os.Stdout.WriteString("waiting: 1\n")
	os.Exit(0)
}
