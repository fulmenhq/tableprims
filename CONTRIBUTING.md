# Contributing

tableprims is at the bootstrap stage; see the [README](README.md) for status.
Useful contributions now are concrete conversion examples, questions about the
typed contract and independently derived expected results.

## Ground rules

- Use public synthetic inputs or redistributable material with provenance. Do
  not submit private data, secrets or redacted production examples. Follow the
  [Sensitive Local Data Policy](https://github.com/3leaps/oss-policies/blob/main/SENSITIVE-LOCAL-DATA.md).
- Dependencies must carry a permissive license accepted by `deny.toml`.
  Copyleft licenses are not accepted, and an unknown license fails the check.
- Proposed changes need maintainer review.

## Checks

Install Rust 1.88.0 or newer and Make, then run:

```sh
make check
```

This checks formatting, runs Clippy with warnings denied, runs the tests, runs
the built `tablebolt` binary and confirms `VERSION` matches `Cargo.toml`.

Before pushing, run `make prepush`. It adds `make deny`, which needs
[cargo-deny](https://github.com/EmbarkStudios/cargo-deny), and `make msrv`,
which needs the Rust 1.88.0 toolchain (`rustup toolchain install 1.88.0`).

## License

Contributions are licensed under MIT OR Apache-2.0. AI-assisted work is welcome
under human supervision and the attribution rules in [AGENTS.md](AGENTS.md).
