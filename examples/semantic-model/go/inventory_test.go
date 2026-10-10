// The semantic-model example from Go: the inventory, and the claim under Acme's relation.
package semanticmodeltest

import (
	"math/big"
	"testing"

	"example.com/semanticmodeltest/inventory"
)

// TEST0343: the inventory and Acme's claim from Go
func Test0343_TheInventoryAndAcmesClaimFromGo(t *testing.T) {
	items := []inventory.Item{{Sku: "a", Count: big.NewInt(3)}, {Sku: "b", Count: big.NewInt(5)}}
	found, err := inventory.Lookup("b", items)
	if err != nil || !found.Valid || found.Value.Int64() != 5 {
		t.Errorf("lookup b: %v, %v", found, err)
	}
	c := inventory.Assurance().Claim("Inventory.lookup_linear")
	if c == nil || c.Relation != "acme.cost-bound" || c.Status != "proved" {
		t.Errorf("lookup_linear: %+v", c)
	}
}
