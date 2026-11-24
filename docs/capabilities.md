# Capabilities

tableprims is implemented twice: a Rust library with an XLSX adapter crate,
and a pure-Go package. The two share the reading of documents, values and
rows; two capabilities are Rust only. Each implementation surface reports
what it implements and answers, through its public API, for what it does
not. A repository suite checks those answers.

## Identifiers

| Capability          | Operation                                        | Go package `table` | Rust `tableprims` | Rust `tableprims-xlsx` |
| ------------------- | ------------------------------------------------ | ------------------ | ----------------- | ---------------------- |
| `read-schema`       | Read a schema document                           | yes                | yes               | no                     |
| `read-policy`       | Read a policy document                           | yes                | yes               | no                     |
| `read-rows`         | Read and check rows under a schema               | yes                | yes               | no                     |
| `quantize`          | Quantize a decimal                               | yes                | yes               | no                     |
| `binary64-decision` | Decide whether an exact number can be a binary64 | no                 | yes               | no                     |
| `xlsx-write`        | Write an XLSX workbook                           | no                 | no                | yes                    |

`binary64-decision` and `xlsx-write` are declared Rust-only in
`.fulmen/repo.yaml`. The core Rust crate never reports `xlsx-write`; an
application that uses both Rust crates composes their two lists.

## API

| Surface                | Report                            | Answer                         |
| ---------------------- | --------------------------------- | ------------------------------ |
| Go `table`             | `table.Capabilities() []string`   | `table.Require(id) error`      |
| Rust `tableprims`      | `tableprims::capabilities()`      | `tableprims::require(id)`      |
| Rust `tableprims-xlsx` | `tableprims_xlsx::capabilities()` | `tableprims_xlsx::require(id)` |

The report is fixed at build time and lists identifiers in the order of the
table above. Each answer comes from the same fixed list as its report: no
error for a capability the surface implements, and otherwise a capability
error whose disposition is

- `unsupported`: a capability of tableprims that this surface does not
  implement, such as `xlsx-write` asked of the Go package; or
- `unknown`: an identifier that names no capability.

The error is `*table.CapabilityError` in Go and
`tableprims::CapabilityError` in Rust. Its text is `capability-unsupported`
or `capability-unknown` and the identifier, quoted by the language's own
string quoting (`strconv.Quote` in Go, `Debug` in Rust); the two agree for
printable ASCII. Compare the disposition, not the text.

A capability error is not a refusal of the Tabular Exchange Standard. A
refusal says that a document, a value or a row cannot be converted under a
policy; a capability error is answered before anything is read, and it has
none of the standard's categories or rules. Call `Require` before choosing
an operation.

## The capability suite

`conformance/capabilities/v0/capabilities.json` belongs to this repository.
It is not part of the standard and is kept apart from the standard's
vectors under `third_party/`. It names the six capabilities, the report of
each of the three surfaces, and 21 cases: every capability on every surface,
and one unknown identifier per surface, each with its expected disposition
(`supported`, `unsupported` or `unknown`).

- Go (`table/capability_test.go`) runs the 7 cases of the Go package and the
  Rust tests (`crates/tableprims-xlsx/tests/capabilities.rs`) the 14 of the
  two Rust surfaces.
- Each report must equal the suite's list exactly, in order, with no missing
  or repeated identifier.
- A `supported` case also runs the operation through the public API and
  checks its result; reporting a capability is not enough.
- An `unsupported` or `unknown` case calls the surface's `require` and
  compares the disposition and the identifier. In Go it also checks that the
  error is not a `*table.Refusal`.
- Both tests check that every surface has a case for every capability and
  one unknown control, and that the package's own list of known identifiers
  equals the suite's.
- The Go test checks that the Rust-only and Go-only extensions declared in
  `.fulmen/repo.yaml` are exactly the capabilities the suite gives to one
  language and not the other.
- Each test alters the expectations in several ways, such as a capability on
  the wrong surface or a missing report entry, and requires a disagreement.

Adding a capability, or implementing one on another surface, changes the
suite, both implementations' registries and, for a per-language capability,
`.fulmen/repo.yaml`, in the same pull request.
