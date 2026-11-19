---
id: "ADR-0001"
title: "A library first, in Rust and then pure Go, under one version"
status: "proposed"
date: "2026-11-19"
relates-to:
  - "PDR-0001-independent-expected-results.md"
  - "DDR-0002-typed-table-and-document-formats.md"
---

# ADR-0001: A Library First, in Rust and Then Pure Go, Under One Version

## Context

Typed tabular conversion is needed inside other programs as well as from a
shell, and in more than one language. A command-line tool alone would force
every consumer through a process boundary. Two unrelated implementations
would drift apart. A Go library that links the Rust one through cgo would not
be usable where cgo is unavailable.

## Decision

1. **The library is the product; the tool is thin.** Conversion semantics and
   policy live in `crates/tableprims`. A format adapter is a separate crate
   that depends on it; `crates/tableprims-xlsx` is the first. Argument
   parsing, terminal output, file publication and exit codes live in
   `crates/tableprims-cli`, which builds the one binary, `tablebolt`.
2. **The core library has no dependencies.** A dependency that only an
   adapter or the tool needs is declared there. `make deps-check` fails when
   the library gains one.
3. **Backend types stay out of public interfaces.** An adapter's public
   interface is expressed in the library's types and its own. The library
   that reads or writes a file format can be replaced without changing a
   caller.
4. **Rust is the first implementation; Go is the second and is pure Go.** The
   Go module is the repository root, `github.com/fulmenhq/tableprims`, and
   builds with `CGO_ENABLED=0` and uses the standard library only. It offers
   a native library interface. It does not wrap the Rust library and does not
   add a second command-line tool.
5. **One repository, one version, one tag.** A release is a single tag that
   releases the Rust crates and the Go module together. The tag tooling
   refuses a commit that has no root `go.mod`.
6. **A surface is planned until it is shown to work.** The README lists each
   language surface with its status. A commitment to build a surface is never
   reported as that surface existing.
7. **Both implementations answer to the same expected results.** They are
   compared on typed values, counts and refusals, as
   [PDR-0001](PDR-0001-independent-expected-results.md) describes. A
   difference in what an implementation supports is published and its refusal
   is tested; it is never left implicit.
8. **TypeScript and Python are bindings to the Rust library**, not further
   implementations.

The four-language shape follows the multi-language commitment for this family
of libraries, [ADR-0018](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/ADR-0018-prims-multi-language-commitment.md)
in 3leaps Crucible, which is itself a proposed record.

## Alternatives

- **Rust with a cgo Go wrapper.** One implementation to maintain, but it
  fails the pure-Go requirement.
- **Two independent products.** Each could move at its own pace, with no
  single contract between them.
- **Both implementations at once.** It doubles the work before the first
  adapter has shown what the contract must say.

## Consequences

- Go users wait for the second implementation, and no release tag can be
  made until the Go module exists.
- Every adapter crate adds a version site and a row in the dependency rule.
- Rules about files on disk bind the tool, not the library: a caller of the
  library that supplies its own streams has none of the tool's publication
  obligations ([DDR-0003](DDR-0003-publishing-output-files.md)).
