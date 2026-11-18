# Changelog

All notable changes to this project are documented in this file. The format
follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Cargo workspace with the `tableprims` library crate and the `tableprims-cli`
  package, which builds the `tablebolt` binary.
- `tablebolt version` and `tablebolt capabilities`, each with `--json`. The
  capability list is empty: no conversion is implemented.
- Library module `exact`: exact integers and fixed-scale decimals of up to 38
  digits with a strict textual grammar, checked addition and quantization
  with five rounding modes.
- Library module `table`: schemas of uniquely identified columns, typed values
  and a row check that locates the first violation by row index and column
  identifier.
- Library module `temporal`: calendar dates, local timestamps and
  fixed-offset timestamps with a declared fractional-second precision, kept
  as written.
- Library module `ndjson`: rows as newline-delimited JSON arrays of `null`,
  booleans and strings, read one line at a time under a line length limit.
  Bare numbers and nested values are refused.
- Library module `binary64`: decides whether an exact integer or decimal can
  be stored as a binary64 number and recovered, and returns the value to
  store. At most 15 significant digits; integers must be exact in binary.
- Library module `json`: a strict reader for small JSON documents that
  refuses repeated object keys, unpaired surrogates and any number other than
  a plain non-negative integer, under a nesting depth limit.
- Library module `documents`: schema and policy documents as strict JSON
  objects with a format identifier. Unknown members are refused at every
  depth. A policy names the exact-number representation, per-column
  quantization, the destination sheet and optional limits.
- Build, format, lint, test, dependency and minimum-Rust-version checks.
- `make check-platform` as the native gate: formatting, Clippy, tests, the
  `tablebolt` smoke run, the leaf-dependency rule, a content check for local
  paths, keys and tokens, and version sites. CI runs it on macOS and Windows
  through a local composite action.
- `make check` as the full gate CI runs on Linux: `check-platform` plus
  workflow digest pins, release tooling controls and cargo-deny.
- `make prepush` adds the minimum-Rust-version build, YAML and workflow lint
  and tool controls to `check`. `make pr-final` adds a required
  public-surface scan, including the PR title and body, to `check`.
- `VERSION` as the single source of truth, with `make version-set`,
  `version-patch`, `version-minor` and `version-major`.
- Release tag tooling and its controls: committed public signing pins and
  fingerprint anchors, a pinned tagger identity, signed-tag creation and
  verification, and a release checklist. No release has been made.

### Changed

- CI runs at the minimum supported Rust version on Linux x86_64 and aarch64,
  macOS arm64 and Windows x86_64 and aarch64; `make msrv` is now
  `make msrv-check`.
