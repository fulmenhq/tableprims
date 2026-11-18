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
