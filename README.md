# tableprims

Typed tabular conversion library, with a thin command-line tool named
`tablebolt`.

**Lifecycle phase**: `experimental`

## Status

This repository is at the bootstrap stage. **No conversion is implemented.**
The library reports an empty conversion list and `tablebolt` offers only
`version` and `capabilities`. Nothing here should be used to convert data yet.

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

| Path                    | Contents                                         |
| ----------------------- | ------------------------------------------------ |
| `crates/tableprims`     | Rust library: conversion semantics and policy    |
| `crates/tableprims-cli` | Thin CLI package; builds the `tablebolt` binary  |

## Language surfaces

| Surface    | Package                           | Status  |
| ---------- | --------------------------------- | ------- |
| Rust       | `tableprims` (crates.io)          | planned |
| Go         | `github.com/fulmenhq/tableprims`  | planned |
| TypeScript | to be named                       | planned |
| Python     | `tableprims` (PyPI)               | planned |

Rust is the first implementation. The Go surface is a pure-Go library
(`CGO_ENABLED=0`) sharing the same conformance vectors. No package has been
published.

## Development

Requires Rust 1.88.0 or newer, Make and Python 3.11 or newer.

```sh
make check-platform  # format check, lint, tests, CLI smoke, dependency, content and version checks
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
