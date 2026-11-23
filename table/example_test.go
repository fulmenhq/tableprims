// SPDX-License-Identifier: MIT OR Apache-2.0

package table_test

import (
	"errors"
	"fmt"
	"io"
	"strings"

	"github.com/fulmenhq/tableprims/table"
)

func convert(schemaBytes, policyBytes []byte, input io.Reader) error {
	schema, err := table.ReadSchema(schemaBytes)
	if err != nil {
		return err
	}
	policy, err := table.ReadPolicy(policyBytes)
	if err != nil {
		return err
	}
	plan, err := table.NewPlan(schema, policy) // a nil policy quantizes nothing
	if err != nil {
		return err
	}
	rows := table.NewRowReader(input, plan, 1<<20)
	for {
		row, err := rows.Next()
		if err == io.EOF { // compare with ==: a read failure can wrap io.EOF
			return nil
		}
		if err != nil {
			// The first refusal ends the input; Next returns it again.
			return err
		}
		fmt.Println(row.Index, row.Values)
	}
}

func ExampleNewRowReader() {
	schema := []byte(`{"format": "tabular-exchange.schema/0", "columns": [
		{"id": "sku", "label": "SKU", "type": "string", "nullable": false},
		{"id": "amount", "label": "Amount", "type": "decimal",
		 "precision": 12, "scale": 2, "nullable": true}]}`)
	policy := []byte(`{"format": "tabular-exchange.policy/0",
		"exact_numbers": "require-exact"}`)
	input := strings.NewReader("[\"0042\",\"19.90\"]\n[\"0043\",null]\n[\"0044\",\"1.5\"]\n")
	err := convert(schema, policy, input)
	var refusal *table.Refusal
	if errors.As(err, &refusal) {
		row, _ := refusal.Row()
		column, _ := refusal.Column()
		fmt.Println(refusal.Category(), refusal.Rule(), row, column)
	}
	// Output:
	// 0 [0042 19.90]
	// 1 [0043 null]
	// schema-mismatch exact-scale-mismatch 2 amount
}
