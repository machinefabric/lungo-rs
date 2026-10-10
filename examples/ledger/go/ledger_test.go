// The ledger example from Go: the proved reducer, and the claims the package carries.
package ledgertest

import (
	"math/big"
	"strings"
	"testing"

	"example.com/ledgertest/ledger"
)

func deposit(n int64) ledger.Event  { return ledger.EventDeposit{Amount: big.NewInt(n)} }
func withdraw(n int64) ledger.Event { return ledger.EventWithdraw{Amount: big.NewInt(n)} }

// TEST0326: the proved ledger from Go
func Test0326_TheProvedLedgerFromGo(t *testing.T) {
	balance, err := ledger.Replay([]ledger.Event{deposit(100), withdraw(30), deposit(5)})
	if err != nil || !balance.Valid || balance.Value.Int64() != 75 {
		t.Errorf("replay: %v, %v", balance, err)
	}
	overdraft, err := ledger.Replay([]ledger.Event{withdraw(1), deposit(100)})
	if err != nil || overdraft.Valid {
		t.Errorf("an overdraft was accepted: %v, %v", overdraft, err)
	}
	a := ledger.Assurance()
	c := a.Claim("Ledger.replay_solvent")
	if c == nil || c.Status != "proved" || strings.Join(c.Specifications, ",") != "Ledger.Solvent" {
		t.Errorf("replay_solvent: %+v", c)
	}
}
