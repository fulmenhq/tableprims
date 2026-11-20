---
id: "ADR-0002"
title: "The conversion result is the record; process integration stays at the tool"
status: "accepted"
date: "2026-11-19"
relates-to:
  - "DDR-0003-publishing-output-files.md"
  - "DDR-0002-typed-table-and-document-formats.md"
  - "DDR-0004-tabular-exchange-standard.md"
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

1. **A run reports its outcome in one result document.** It states the
   status, the identity of the run, digests and lengths of the inputs that
   were read to their end, and either the digest, length and per-column
   counts of the output or a located refusal. Cell content never appears in
   it. A run that ends before its result file can be opened or published
   leaves none: a usage error, a result name that is taken or cannot be
   written, and a run ended by a signal before the result is published.
2. **The result document is the authoritative record.** A conversion is
   complete only when a result with status `ok` matches the converted file:
   a reader checks that the result names the run and inputs it expects and
   that its digest and length are those of the converted file. How the two files are published so that this holds is
   [DDR-0003](DDR-0003-publishing-output-files.md). Two files cannot be
   published in one step, so only the second, the result, carries meaning.
3. **Supervised runs adopt the process-run contract.** The tool will report
   a run's start, progress and end to a supervisor through the process-run
   contract of 3leaps Crucible (a proposed record there,
   [ADR-0006](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/ADR-0006-process-run-contract.md)),
   beginning with telemetry only, and not through a format of this project's
   own. This is not implemented: the tool emits no progress or telemetry
   stream and offers no control endpoint. Any control exchange added later is
   local and authenticated; there is no remote listener and no daemon.
4. **Nothing required depends on telemetry.** Telemetry is optional and
   fail-open: its failure does not fail a conversion, and its presence is
   never evidence that a conversion completed. The result document carries
   no dependency on it.
5. **Library errors are typed; exit codes belong to the tool.** The core
   library returns typed errors. An adapter gives each of its errors a
   category and a stable rule identifier, and the tool does the same for
   document and file errors. The tool alone maps categories to exit codes,
   using existing entries of the
   [Fulmen exit-code catalog](https://github.com/fulmenhq/crucible/blob/95a89231a048edef830351de43695cc0ef58c0e8/config/library/foundry/exit-codes.yaml).
   The mapping is in [docs/xlsx-write.md](../xlsx-write.md). Process exit
   codes and transport status codes do not appear in library interfaces.
6. **The result document is the tool's artifact.** Its digests, canonical
   serialization and generated run identity are dependencies of the tool
   crate only. A library caller working on streams receives typed outcomes,
   counts and located errors, and has no result file.
7. **Cancellation is a library input and, for now, nothing more.** The
   adapter checks a caller-supplied flag before each row. The tool installs
   no signal handler: a signal ends the process and the supervisor sees death
   by signal. A signal before the result is published leaves no result and
   can leave a converted file without one. A signal after it is published
   does not undo a result that matches. Status `cancelled` and exit codes 130
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

- A consumer that did not supervise a run does not need its exit status. It
  reads the result file and checks it against the converted file and against
  the run and inputs it expected. A result file that is absent or does not
  match means the conversion is not complete; it does not say why.
- A supervisor learns how the process ended from the process, separately
  from the result file. The two can differ: a run killed after publishing a
  matching result has completed its conversion.
- A run ended by a signal before the result is published leaves a state that
  is recognisable under DDR-0003, with nothing recording the cause.
- Until rule 3 is implemented, a supervisor gets no progress from a long
  conversion.
- The fields a consumer in another project needs in order to interpret a
  result (status, run and input binding, output digest and length, and the
  matching rule) are candidates for a shared contract; see
  [DDR-0002](DDR-0002-typed-table-and-document-formats.md). File-system
  mechanics stay in this repository.
