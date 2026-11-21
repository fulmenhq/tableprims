# Go package

`github.com/fulmenhq/tableprims/table` is the Go implementation of the
tableprims library. It reads the documents, values and rows of the Tabular
Exchange Standard, contract revision `tabular-exchange/0.1.0`, published in
[fulmenhq/crucible](https://github.com/fulmenhq/crucible). It is pure Go: the
standard library only, no cgo, and no wrapper of the Rust library
([ADR-0001](decisions/ADR-0001-library-first-rust-and-go.md)). The module is the
repository root and is released with the Rust crates under one tag; no
release has been made.

## What it covers

| Operation                         | Go | Rust library | `tablebolt` |
| --------------------------------- | -- | ------------ | ----------- |
| Read a schema or policy document  | yes | yes         | yes         |
| Value grammar and row checks      | yes | yes         | yes         |
| Quantize decimal columns          | yes | yes         | yes         |
| Read the rows encoding            | yes | yes         | yes         |
| Decide a binary64 representation  | no  | yes         | yes         |
| Write an XLSX workbook            | no  | adapter crate | yes       |
| Result document, exit codes       | no  | no          | yes         |
| Process-run telemetry             | no  | no          | yes         |

There is no Go command-line tool. The Go package applies none of the rules
of a destination: the worksheet name, the row and empty-range ceilings of
the workbook and the cell length are checked by the XLSX adapter. A policy's
`sheet` and `limits` are read and exposed.

## Use

```go
schema, err := table.ReadSchema(schemaBytes)
policy, err := table.ReadPolicy(policyBytes)
plan, err := table.NewPlan(schema, policy) // a nil policy quantizes nothing
rows := table.NewRowReader(input, plan, 1<<20)
for {
	row, err := rows.Next()
	if err == io.EOF {
		break
	}
	var refusal *table.Refusal
	if errors.As(err, &refusal) {
		// refusal.Category(), refusal.Rule(), refusal.Row(), refusal.Column()
	}
	_ = row.Values // typed: Null, Boolean, Text, Integer, Decimal, Date, ...
}
```

Every refusal is a `*table.Refusal`. It names the category and the rule
identifier as the standard registers them, and the zero-based row and the
column identifier when the refusal has them; `Row` and `Column` say whether
it has one. A message never contains document or cell content. For the same
input, the category, the rule and the location are the ones `tablebolt`
reports.

## Conformance

`go test ./...` runs the same 477 vectors and fixtures that the Rust tests
run, from the files vendored under
[`third_party/crucible/`](../third_party/crucible/PROVENANCE.md), after checking
the SHA-256 of every vendored file. It compares the verdict, category, rule,
row and column of each outcome; for the 77 quantize vectors it also compares
the quantized text and both change flags. It fails when a suite runs fewer
cases than the release holds. The policy fixtures are run as released: the
Go package does not require a worksheet name. As in Rust, the 92 conversion
result fixtures and the 70 matching vectors are not run.

## Differences from `tablebolt` outside the vectors

- A failure of the underlying reader is `io-failure`, `read-failed`, with the
  cause available through `errors.Unwrap`. `tablebolt` maps a full disk, a
  quota, a file size limit and exhausted memory to `resource-limit` rules.
- `NewRowReader` takes the line limit as an argument and has no default;
  `tablebolt` uses 1 MiB unless the policy sets `max_line_bytes`. The reader
  may read up to 64 KiB past the limit before it refuses the line.
- The row reader applies no row limit: `too-many-rows` is a rule of the
  workbook. `tablebolt` refuses a row past `max_rows` before it checks that
  row against the schema; a Go caller that counts rows itself sees the row's
  schema refusal first.
- `ReadSchema` and `ReadPolicy` refuse a document larger than 16 MiB with
  `schema-too-large` or `policy-too-large`, as `tablebolt` does for its files.
- A schema or plan built in code rather than read from a document is checked
  too: an unknown type, an impossible precision or an unknown rounding mode is
  refused with the rule its document form would get.

## Build

```sh
make check-go       # gofmt, go vet and a build with CGO_ENABLED=0
make test-go        # the tests, the conformance vectors included
make test-go-race   # the tests under the race detector (needs cgo)
```

Go 1.26 or newer. `make deps-check` fails if the module gains a dependency,
uses cgo, or imports a package outside the standard library on any of the
platforms it checks.
