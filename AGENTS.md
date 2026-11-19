# tableprims contributor agent guide

Read `README.md` and `CONTRIBUTING.md` before changing code, and the records
under `docs/decisions/`. Read local guidance when
present.

## Current phase

Pre-release. One conversion is implemented: `tablebolt xlsx write`, described
in `docs/xlsx-write.md`. Nothing has been released.

- Do not add placeholder crates, modules or APIs that imply support which does
  not exist. `conversions()` lists implemented behaviour only.
- Keep conversion semantics and policy in `crates/tableprims`. Keep argument
  parsing, terminal output and exit codes in `crates/tableprims-cli`.
- The library crate takes no dependency that only the CLI needs.
- Expected results for conformance vectors are authored independently of this
  implementation. Never change an expectation to make the implementation pass.
- Do not claim tests, platforms or package releases that were not executed.

## Public content

Follow the [Sensitive Local Data Policy](https://github.com/3leaps/oss-policies/blob/dee9d320f5b7045a44c92b200cc1c92ca83ae843/SENSITIVE-LOCAL-DATA.md).
Private inputs, planning, identifiers and operating notes stay outside the
working tree, including ignored paths. Use synthetic examples. Treat commit
messages, branches and PR text as permanent public content.

## Checks

Run `make check` before committing, `make prepush` before pushing and
`make pr-final` before opening or updating a PR.

- `make check` is everything CI runs on one host: formatting, Clippy, tests,
  the `tablebolt` smoke run, the dependency rule (`make deps-check`), the
  content check, version sites, workflow digest pins and cargo-deny.
- `make prepush` adds the minimum supported Rust version build
  (`make msrv-check`), YAML and workflow lint, the tool controls and the
  public-surface scan, which is skipped when no catalog is configured.
- `make pr-final` requires the public-surface scan, including the PR title
  and body.

tableprims is a leaf: it depends on no other prim. Change the version only
through `make version-set`, `make version-patch`, `make version-minor` or
`make version-major`. A change to a workflow or local action updates
`config/release/release-workflow.sha256` in the same change. Release steps are
in `RELEASE_CHECKLIST.md`; no release has been made.

Use the public attribution footer on commits and PR bodies:

```text
Role: <bare-role>
Co-authored-by: <model> <noreply@3leaps.net>
Committer-of-Record: @3leapsdave
```
