// The sort example from Go: the same proved sorts, and the claims the package carries.
package sorttest

import (
	"math/big"
	"math/rand"
	"slices"
	"strings"
	"testing"

	"example.com/sorttest/sorting"
)

func nats(xs []int64) []*big.Int {
	out := make([]*big.Int, len(xs))
	for i, x := range xs {
		out[i] = big.NewInt(x)
	}
	return out
}

// TEST0311: the proved sort sorts in Go
func Test0311_TheProvedSortSortsInGo(t *testing.T) {
	r := rand.New(rand.NewSource(311))
	for n := 0; n < 200; n += 7 {
		xs := make([]int64, n)
		for i := range xs {
			xs[i] = r.Int63n(1000)
		}
		got, err := sorting.Sort(nats(xs))
		if err != nil {
			t.Fatal(err)
		}
		slices.Sort(xs)
		if !slices.EqualFunc(got, nats(xs), func(a, b *big.Int) bool { return a.Cmp(b) == 0 }) {
			t.Fatalf("sort of %d numbers: %v", n, got)
		}
	}
	ranked, err := sorting.Rank([]sorting.Entry{
		{Name: "ada", Score: big.NewInt(3)},
		{Name: "bo", Score: big.NewInt(9)},
		{Name: "cy", Score: big.NewInt(3)},
	})
	if err != nil {
		t.Fatal(err)
	}
	names := []string{}
	for _, e := range ranked {
		names = append(names, e.Name)
	}
	if strings.Join(names, ",") != "bo,ada,cy" {
		t.Errorf("rank: %v", names)
	}
}

// TEST0312: the Go package carries the sorts' claims
func Test0312_TheGoPackageCarriesTheSortsClaims(t *testing.T) {
	a := sorting.Assurance()
	for _, name := range []string{"Sorting.sort_satisfies", "Sorting.rank_satisfies"} {
		c := a.Claim(name)
		if c == nil || c.Status != "proved" || c.Relation != "lungo.satisfies" || len(c.Assumptions) != 0 {
			t.Errorf("%s: %+v", name, c)
		}
	}
	if e := a.Export("Sorting.rank"); e == nil || strings.Join(e.Claims, ",") != "Sorting.rank_satisfies,Sorting.rank_stable" {
		t.Errorf("rank: %+v", e)
	}
}
