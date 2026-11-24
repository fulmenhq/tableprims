# Contributing

tableprims is experimental; see the [README](README.md) for
status and [docs/decisions](docs/decisions/README.md) for the decisions that
bind changes.
Useful contributions now are concrete conversion examples, questions about the
typed contract and independently derived expected results.

## Ground rules

- Use public synthetic inputs or redistributable material with provenance. Do
  not submit private data, secrets or redacted production examples. Follow the
  [Sensitive Local Data Policy](https://github.com/3leaps/oss-policies/blob/dee9d320f5b7045a44c92b200cc1c92ca83ae843/SENSITIVE-LOCAL-DATA.md).
- Dependencies must carry a permissive license accepted by `deny.toml`.
  Copyleft licenses are not accepted, and an unknown license fails the check.
- The Rust library and the Go package `table` are locked: a change to the
  shared document, value or row behaviour, or to the vendored vectors, changes
  both implementations and their tests in the same pull request.
- Proposed changes need maintainer review.

## Checks

Install Rust 1.88.0 or newer, Go 1.26 or newer, Make, Git and Python 3.11 or
newer, then run:

```sh
make check-platform
```

This checks formatting, runs Clippy with warnings denied, runs the Rust and
Go tests (the Go ones with `CGO_ENABLED=0`), runs
the built `tablebolt` binary, and checks the dependency rule, tracked
files for local paths, keys and tokens, and that every version site matches
`VERSION`.

`make check` is everything CI runs on one host. It adds the workflow digest
pins, which need [PyYAML](https://pyyaml.org/), and `make deny`, which needs
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny).

Before pushing, run `make prepush`. It adds `make msrv-check`, which needs the
Rust 1.88.0 toolchain (`rustup toolchain install 1.88.0`), `make lint-yaml`
(`yamlfmt` and `yamllint`), `make lint-workflows` (`actionlint`), and the tool
controls, which need Go. `make doctor DOCTOR_SCOPE=dev` reports host
tool versions.

Every workflow and local action is pinned by digest in
`config/release/release-workflow.sha256`. A change to one updates its pin line
in the same change and is reviewed as a trust-file change. Change the version
only through the `make version-*` targets.

## License

Contributions are licensed under MIT OR Apache-2.0. AI-assisted work is welcome
under human supervision and the attribution rules in [AGENTS.md](AGENTS.md).
