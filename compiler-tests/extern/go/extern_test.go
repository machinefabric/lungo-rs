package ext

import (
	"math/big"
	"testing"

	"example.com/ext/consumer"
	"example.com/ext/provider"
)

// TEST0005: A value made by one program is used by the other: both run on the one runtime.
func Test0005_ValuesPassBetweenPrograms(t *testing.T) {
	p, err := provider.MkPos(big.NewInt(3))
	if err != nil || !p.Valid {
		t.Fatalf("mkPos 3 = %v, %v", p, err)
	}
	d, err := consumer.Double(p.Value)
	if err != nil {
		t.Fatal(err)
	}
	v, err := provider.Value(d)
	if err != nil || v.Int64() != 6 {
		t.Fatalf("value (double 3) = %v, %v", v, err)
	}
	q, err := provider.MakePair(big.NewInt(1), "apples")
	if err != nil {
		t.Fatal(err)
	}
	c, err := consumer.Count(d, q)
	if err != nil {
		t.Fatal(err)
	}
	s, err := consumer.Describe(c)
	if err != nil || s != "apples: 7" {
		t.Fatalf("describe = %q, %v", s, err)
	}
}
