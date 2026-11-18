# tableprims contributor agent guide

Read `README.md` and `CONTRIBUTING.md` before changing code, and the records
under `docs/decisions/` when that directory exists. Read local guidance when
present.

## Current phase

Bootstrap. The workspace builds, and the `tablebolt` binary reports its version
and an empty conversion list. No conversion is implemented.

- Do not add placeholder crates, modules or APIs that imply support which does
  not exist. `conversions()` lists implemented behaviour only.
- Keep conversion semantics and policy in `crates/tableprims`. Keep argument
  parsing, terminal output and exit codes in `crates/tableprims-cli`.
- The library crate takes no dependency that only the CLI needs.
- Expected results for conformance vectors are authored independently of this
  implementation. Never change an expectation to make the implementation pass.
- Do not claim tests, platforms or package releases that were not executed.

## Public content

Follow the [Sensitive Local Data Policy](https://github.com/3leaps/oss-policies/blob/main/SENSITIVE-LOCAL-DATA.md).
Private inputs, planning, identifiers and operating notes stay outside the
working tree, including ignored paths. Use synthetic examples. Treat commit
messages, branches and PR text as permanent public content.

## Checks

Run `make check` before committing and `make prepush` before publication.
`make prepush` adds the dependency license and advisory check and the minimum
supported Rust version build.

Use the public attribution footer on commits and PR bodies:

```text
Role: <bare-role>
Co-authored-by: <model> <noreply@3leaps.net>
Committer-of-Record: @3leapsdave
```
