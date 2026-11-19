---
id: "ADR-0002"
title: "The conversion result is the record; process integration stays at the tool"
status: "proposed"
date: "2026-11-19"
relates-to:
  - "DDR-0003-publishing-output-files.md"
  - "DDR-0002-typed-table-and-document-formats.md"
  - "../xlsx-write.md"
---

# ADR-0002: The Conversion Result Is the Record; Process Integration Stays at the Tool

## Context

A conversion run has an outcome that other programs act on: whether it
finished, what it wrote, and what it changed or refused. That outcome can be
reported through an exit status, progress output, a telemetry stream or a
file. These fail in different ways. An exit status is seen only by the
process that waited for it. A stream can end without saying why. A program
that later finds a converted file has neither.

## Decision

1. **Every run of the tool produces one result document.** It states the
   status, the identity of the run, digests and lengths of the inputs that
   were read to their end, and either the digest, length and per-column
   counts of the output or a located refusal. Cell content never appears in
   it.
2. **The result document is the authoritative record.** A conversion is
   complete only when a result with status `ok` matches the converted file.
   How the two files are published so that this holds is
   [DDR-0003](DDR-0003-publishing-output-files.md). Two files cannot be
   published in one step, so only the second, the result, carries meaning.
3. **Nothing required depends on telemetry.** The tool emits no progress or
   telemetry stream and offers no control endpoint. If either is added it is
   optional and fail-open: its failure does not fail a conversion, and its
   presence is never evidence that a conversion completed.
4. **Supervised-run telemetry, if added, uses the existing contract.** It
   follows the process-run contract of 3leaps Crucible
   ([ADR-0006](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/ADR-0006-process-run-contract.md))
   and is not a format of this project's own. Any control exchange is local
   and authenticated; there is no remote listener and no daemon.
5. **Library errors are typed; exit codes belong to the tool.** The library
   and adapters return errors with a category and a stable rule identifier.
   The tool alone maps them to exit codes, using existing entries of the
   [Fulmen exit-code catalog](https://github.com/fulmenhq/crucible/blob/95a89231a048edef830351de43695cc0ef58c0e8/config/library/foundry/exit-codes.yaml).
   The mapping is in [docs/xlsx-write.md](../xlsx-write.md). Process exit
   codes and transport status codes do not appear in library interfaces.
6. **The result document is the tool's artifact.** Its digests, canonical
   serialization and generated run identity are dependencies of the tool
   crate only. A library caller working on streams receives typed outcomes,
   counts and located errors, and has no result file.
7. **Cancellation is a library input and, for now, nothing more.** The
   adapter checks a caller-supplied flag before each row. The tool installs
   no signal handler: a signal ends the process, the supervisor sees death by
   signal, and no result is written. Status `cancelled` and exit codes 130
   and 143 are reserved and not produced.

## Alternatives

- **A manifest or directory commit protocol** for the two output files. It
  gives a reader one name to check, at the cost of a third artifact and a
  recovery procedure. The result file already binds the output by digest and
  length, so it serves as the marker.
- **Progress as ad hoc JSON on standard error.** Simple to add, and one more
  format for every supervisor to learn.
- **A long-running service.** It adds lifecycle and authority questions that
  a batch conversion does not have.

## Consequences

- A consumer that did not supervise a run reads the result file and nothing
  else.
- A run ended by a signal leaves no result. The state it leaves on disk is
  recognisable under DDR-0003, but nothing records the cause.
- A supervisor gets no progress from a long conversion.
- The fields a consumer in another project needs in order to interpret a
  result (status, run and input binding, output digest and length, and the
  matching rule) are candidates for a shared contract; see
  [DDR-0002](DDR-0002-typed-table-and-document-formats.md). File-system
  mechanics stay in this repository.
