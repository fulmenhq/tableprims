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
- Build, format, lint, test, dependency and minimum-Rust-version checks.
- `make check` as the single gate CI runs through one composite action:
  formatting, Clippy, tests, the `tablebolt` smoke run, the leaf-dependency
  rule, a content check for local paths, keys and tokens, version sites,
  workflow digest pins and cargo-deny. `make prepush` and `make pr-final` add
  the minimum-Rust-version build, YAML and workflow lint, tool controls and a
  maintainer public-surface scan.
- `VERSION` as the single source of truth, with `make version-set`,
  `version-patch`, `version-minor` and `version-major`.
- Release tag tooling and its controls: committed public signing pins and
  fingerprint anchors, a pinned tagger identity, signed-tag creation and
  verification, and a release checklist. No release has been made.

### Changed

- CI runs at the minimum supported Rust version on Linux x86_64 and aarch64,
  macOS arm64 and Windows x86_64 and aarch64; `make msrv` is now
  `make msrv-check`.
