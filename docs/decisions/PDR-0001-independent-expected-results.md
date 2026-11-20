---
id: "PDR-0001"
title: "Expected results are authored independently of the implementation"
status: "accepted"
date: "2026-11-19"
relates-to:
  - "ADR-0001-library-first-rust-and-go.md"
---

# PDR-0001: Expected Results Are Authored Independently of the Implementation

## Context

A conversion library is trusted for what it preserves. A test whose expected
output was produced by the code under test shows only that the code has not
changed. Two implementations checked against each other can agree on the same
mistake.

## Decision

1. **An expectation is derived without the implementation.** The expected
   values, counts or refusal of a conformance case are worked out by hand or
   with independent exact arithmetic. They are never a snapshot of this
   implementation's output and are never computed with its policy code.
2. **An expectation is not changed to make the implementation pass.** A
   disagreement is settled by finding which side is wrong.
3. **A case states everything it depends on.** It binds the input, the
   schema, the policy, the expected typed values or refusal, and how the
   expectation was derived.
4. **Output is verified by reading it back independently.** Tests read a
   written workbook with a different parser from the one that wrote it, and
   check numeric cells in the stored XML with exact decimal parsing. Those
   readers are test-only dependencies.
5. **Implementations are compared on meaning, not bytes.** Typed values,
   order, counts and refusals are compared. File bytes are compared only
   where a profile promises canonical bytes; a written workbook is not
   byte-reproducible. A file's digest identifies one run's output.
6. **The same cases judge both implementations**
   ([ADR-0001](ADR-0001-library-first-rust-and-go.md)).
7. **A shared suite is pinned, not followed.** The tests in this repository
   are a small, fast, synthetic set. A conformance suite kept outside the
   repository is used at a fixed revision with content digests, never at a
   moving branch.
8. **A reference engine is a comparator, not the truth.** Agreement with
   another program, or with several, does not establish an expectation.
9. **Unsupported is not a pass.** A case the implementation cannot run is
   reported as unsupported or inconclusive. A refusal the contract requires
   is tested as a refusal.
10. **A check must be able to fail.** A gate that claims to detect a changed
    digit, a dropped row, reordered columns, missing metadata or a truncated
    file has a test that introduces that fault and sees the gate fail. The
    fault is introduced through the path that really produces the output.
11. **Fixtures are synthetic or redistributable.** Private data and redacted
    copies of it are not admitted.

This applies the 3leaps Crucible principle on verification gates,
[EPR-0002](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/EPR-0002-verification-gate-integrity.md),
and adopts the obligations of
[EPR-0005](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/EPR-0005-test-evidence-independence.md)
for this repository. EPR-0005 is a proposed record there; adopting its
obligations here does not change its status.

## Alternatives

- **Snapshot tests.** Cheap to write and they detect change, not error.
- **Agreement with a reference spreadsheet engine.** It measures
  compatibility, and inherits that engine's rounding.

## Consequences

- Expectations cost more to write than snapshots and need a reviewer other
  than the implementer.
- Results from opening a workbook in a spreadsheet application are recorded
  separately from automated results and are not implied by them.
- One external suite is pinned in this repository: the vectors and fixtures
  of the Tabular Exchange Standard from the fulmenhq/crucible release
  `v0.5.1`, under `third_party/crucible/`.
- Of the faults in rule 10, the tests in this repository introduce a changed
  digit and an input that changes between its two reads. The others have no
  such test yet.
