# Release Notes

> **Note:** This file aggregates the latest 3 releases in reverse chronological order.
> For the complete release history, see `CHANGELOG.md`.
> For detailed release documentation, see `docs/releases/`.

---

## v0.1.0 - 2026-11-24

**Status:** First release (experimental)

First release of tableprims: a typed tabular conversion library in Rust and
pure Go, and the `tablebolt` command-line tool, following the Tabular
Exchange Standard, contract revision `tabular-exchange/0.1.0`.

- Rust library, XLSX adapter and `tablebolt xlsx write`.
- Go package `github.com/fulmenhq/tableprims/table`: documents, values,
  quantization and rows; no workbook writing.
- Both implementations meet the same 477 vectors of the standard and are
  locked from this release.
- One signed tag for the Rust crates and the Go module; the crates are not
  published to a registry and no binary is attached.

See [docs/releases/v0.1.0.md](docs/releases/v0.1.0.md).
