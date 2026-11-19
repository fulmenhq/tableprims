---
id: "SecDR-0001"
title: "Inputs are untrusted and bounded; dependencies are permissive and offline"
status: "proposed"
date: "2026-11-19"
relates-to:
  - "../decisions/DDR-0003-publishing-output-files.md"
  - "../decisions/DDR-0002-typed-table-and-document-formats.md"
  - "../xlsx-write.md"
---

# SecDR-0001: Inputs Are Untrusted and Bounded; Dependencies Are Permissive and Offline

## Context

A conversion tool is given files from other parties and writes files that
people open in spreadsheet applications. Input can be malformed or oversized
on purpose. Text in a cell can look like a formula. A parsing or compression
library can bring native code, network access or a licence the project cannot
ship.

## Decision

1. **File content is untrusted; the invocation is trusted.** Rows, schema and
   policy are untrusted, as is any metadata read from a file. Paths and
   limits come only from the caller. No value in any document names a path or
   a network location.
2. **Text cannot become a formula.** Strings are written as string cells and
   are never interpreted as formulas or links. Nothing is evaluated, no macro
   is written and nothing is fetched.
3. **Every input is bounded while it is read.** A line of rows, a schema or
   policy document, the number of rows and columns, the length of a cell, the
   digits of a number, the nesting of a document and the number of
   empty-string ranges each have a limit. A limit is enforced as data is
   read, not by a check beforehand. The limits are listed in
   [docs/xlsx-write.md](../xlsx-write.md).
4. **A policy can set a limit, up to the profile's ceiling and no higher.**
   Exceeding a limit refuses the run with a rule that names it. Running out
   of storage or memory is reported separately from exceeding a configured
   limit.
5. **Diagnostics carry no cell content.** A refusal gives a category, a rule,
   a row and a column.
6. **Conversion is offline.** No crate in the dependency graph is a network
   client; `deny.toml` bans them by name and `make deny` checks it.
7. **The workspace forbids `unsafe` code** in its own crates.
8. **Only permissive licences are accepted.** The allowed list is in
   `deny.toml`. Copyleft licences are not accepted, a dual licence qualifies
   only through a permitted branch, and an unknown licence fails the check.
   The check covers the resolved graph for every supported target, not only
   direct dependencies.
9. **A dependency's features are chosen deliberately.** Default features are
   off for the workbook writer, its version is pinned exactly, and crates
   come only from the public registry. A feature that links a native library
   needs review before it is enabled.
10. **The Go module builds without cgo and uses the standard library only.**
11. **Containment is the caller's.** The library takes a cancellation flag and
    limits on its inputs. It does not limit its own time, memory or scratch
    space; a caller that needs hard limits runs the tool under a supervisor
    that enforces them.
12. **The destination directory is trusted.** Output files are never replaced
    and never removed, as
    [DDR-0003](../decisions/DDR-0003-publishing-output-files.md) sets out.
    Those rules do not hold against a party that can rewrite the directory.
13. **Reading is held to the same standard before it ships.** A reader of
    compressed containers bounds the number of entries and the expanded size
    as it decompresses, bounds sheet dimensions and shared strings, refuses
    macro-bearing files by inspecting content, and refuses formulas unless
    the caller opts in. A formula is never evaluated. No reader ships
    without these.

## Alternatives

- **Rely on the parsing library's defaults.** Less code, and limits that are
  not ours to state or test.
- **Write every value as text.** It removes numeric surprises and does
  nothing about an oversized or malformed input.
- **Neutralise formula-like text by prefixing it.** Common for CSV. It
  changes the value, so it would have to be a policy the result discloses,
  and typed string cells make it unnecessary for XLSX.

## Consequences

- Inputs that other tools accept are refused here, and the refusal is the
  documented behaviour.
- Adding a dependency can fail on a licence deep in its graph.
- The test-only workbook readers are not held to rule 13. They read files the
  tests wrote.
- A run killed by a supervisor leaves temporary files and no result; DDR-0003
  describes what a reader can conclude.
