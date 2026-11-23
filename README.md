# tableprims

Typed tabular conversion library, with a thin command-line tool named
`tablebolt`.

**Lifecycle phase**: `experimental`

## Status

This repository is experimental and has made no release. One conversion is
implemented: `tablebolt xlsx write` writes typed rows to an XLSX workbook; see
[docs/xlsx-write.md](docs/xlsx-write.md). Its rows, schema, policy and result
follow the Tabular Exchange Standard, contract revision
`tabular-exchange/0.1.0`, published in
[fulmenhq/crucible](https://github.com/fulmenhq/crucible); the workbook layout
and the members under the `tableprims` extension namespace are provisional and
may change before the first release. Workbooks have been checked by reading them
back with an independent parser, not by opening them in a spreadsheet
application. A run can report its progress through the process-run contract
of 3leaps Crucible, telemetry only, when given a runtime directory.

The library is implemented twice, in Rust and in pure Go. The Go package,
`github.com/fulmenhq/tableprims/table`, reads the documents, values and rows;
it does not write workbooks and has no command-line tool. See
[docs/go.md](docs/go.md).

The tests run the standard's value, row and document vectors and its table
schema and conversion policy fixtures, 477 of the 639 vectors and fixtures in
the crucible release `v0.5.1`, through the `tablebolt` binary and through the
Go package, and both meet the expectation of every one, each at the stage
where it applies. The two are not identical outside these operations: see
[docs/go.md](docs/go.md#conformance). The conversion result fixtures and the
matching vectors are not run: neither reads results. See
[Vectors of the standard](docs/xlsx-write.md#vectors-of-the-standard).

## What we are building

A reusable library that converts typed tables to and from spreadsheet and
columnar formats under an explicit contract:

- exact integers and decimals never pass through binary floating point;
- strings stay strings, including leading zeros and formula-like text;
- null, empty and missing values stay distinct;
- any change of representation is selected by the caller and disclosed in the
  result, never applied silently.

XLSX export is the first adapter in scope, followed by XLSX import. CSV, Arrow
IPC and Parquet adapters are planned against the same typed contract.

## Layout

| Path                          | Contents                                                         |
| ----------------------------- | ---------------------------------------------------------------- |
| `crates/tableprims`           | Rust library: conversion semantics and policy                    |
| `crates/tableprims-cli`       | Thin CLI package; builds the `tablebolt` binary                  |
| `crates/tableprims-xlsx`      | XLSX write adapter                                               |
| `table`                       | Go package: documents, values and rows (module root `go.mod`)    |
| `third_party/crucible`        | Vectors, fixtures and schemas of the standard, vendored unedited |
| `third_party/3leaps-crucible` | Process-run schemas and examples, vendored unedited              |

## Language surfaces

| Surface    | Package                           | Status  |
| ---------- | --------------------------------- | ------- |
| Rust       | `tableprims` (crates.io)          | planned |
| Go         | `github.com/fulmenhq/tableprims`  | planned |
| TypeScript | to be named                       | planned |
| Python     | `tableprims` (PyPI)               | planned |

Rust is the first implementation. The Go surface is a pure-Go library
(`CGO_ENABLED=0`, standard library only) that reads documents, values and
rows and answers to the same conformance vectors; it does not write
workbooks. No package has been published.

## Development

Requires Rust 1.88.0 or newer, Go 1.26 or newer, Make and Python 3.11 or
newer.

```sh
make check-platform  # format check, lint, Rust and Go tests, CLI smoke, dependency, content and version checks
make check           # everything CI runs: adds workflow digest pins (PyYAML) and cargo-deny
make prepush         # adds the MSRV build, YAML and workflow lint, and the tool controls
make help            # every target
```

See [CONTRIBUTING.md](CONTRIBUTING.md) and [AGENTS.md](AGENTS.md). The release
process is in [RELEASE_CHECKLIST.md](RELEASE_CHECKLIST.md) and the release
trust model in [docs/security/README.md](docs/security/README.md); no release
has been made.

## License

Licensed under either of the [MIT License](LICENSE-MIT) or the
[Apache License, Version 2.0](LICENSE-APACHE), at your option.
