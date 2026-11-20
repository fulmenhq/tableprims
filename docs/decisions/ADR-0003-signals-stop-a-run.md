---
id: "ADR-0003"
title: "SIGINT and SIGTERM stop a run, which says so in its result"
status: "accepted"
date: "2026-11-20"
supersedes: "ADR-0002-conversion-result-and-process-boundary.md (rule 7 only)"
relates-to:
  - "ADR-0002-conversion-result-and-process-boundary.md"
  - "DDR-0003-publishing-output-files.md"
  - "SecDR-0001-untrusted-input-and-dependencies.md"
  - "../xlsx-write.md"
---

# ADR-0003: SIGINT and SIGTERM Stop a Run, Which Says So in Its Result

## Context

[ADR-0002](ADR-0002-conversion-result-and-process-boundary.md) rule 7 made
cancellation a library input and nothing more: the tool installed no signal
handler, a signal ended the process, and the status `cancelled` and exit
codes 130 and 143 were reserved. A supervisor that stops a run therefore saw
death by signal and found no result, and a consumer could not tell a stopped
run from one that crashed.

The library already checks a caller-supplied flag before every row of both
reads. The Tabular Exchange Standard, `tabular-exchange/0.1.0`, defines the
status `cancelled` with the category `cancelled`. The workspace forbids
`unsafe` code, so installing a handler needs a dependency.

## Decision

This record replaces rule 7 of ADR-0002. The other rules of ADR-0002 stand.

1. **The first SIGINT or SIGTERM asks the run to stop.** The tool installs
   handlers for both before it does anything else, whatever disposition it
   inherited. A handler only sets a flag. The flag is checked before every
   row of both reads of the rows and once more after the workbook is written
   and synced, just before it is published.
2. **A stopped run publishes a result that says so, then ends by the
   signal.** The status is `cancelled`, the refusal is category `cancelled`,
   rule `cancelled`, with no row or column. The inputs read to their end are
   bound as in any other result. No workbook is published, and this
   program's temporary files and the workbook writer's scratch files are
   removed. Once that result is published, the process ends by the same
   signal on Unix, so a shell reports 130 after SIGINT and 143 after SIGTERM
   and a shell loop stops as it would for any interrupted program; elsewhere
   it exits with 130 or 143. If the result cannot be published, the run
   fails as any run whose result cannot be written does
   ([DDR-0003](DDR-0003-publishing-output-files.md); exit 54 when its name is
   taken) and does not end by the signal.
3. **After the last check, the normal publication rules apply.** A signal
   that arrives after the check before publishing does not stop the run. The
   run publishes its workbook and its result as any run does, which ends in
   exit 0 unless a name was taken or a write failed in the meantime.
4. **A second SIGINT or SIGTERM ends the process at once** by that signal's
   default action. If the run has not yet published a result it publishes
   none, and a temporary result file can remain. This leaves the state that
   [DDR-0003](DDR-0003-publishing-output-files.md) describes for a run that
   is killed.
5. **Other signals keep their default action.** SIGKILL cannot be handled.
   SIGHUP, SIGQUIT and SIGPIPE are not handled.
6. **The handler is a flag-only dependency of the tool.** `signal-hook`
   with its default features off, in `tableprims-cli` only. The library and
   the adapter keep taking the flag as an input and gain no dependency.

## Verification and platforms

Tests on Unix in `crates/tableprims-cli/tests/xlsx_write.rs` send SIGINT and
SIGTERM while the workbook is written, SIGINT while the rows are read through
a pipe, a second signal while the rows are read, and SIGINT to a run whose
result name is taken meanwhile. They check how the process ended, the result
and what is left on disk. That covers the per-row check of rule 1 and rules
2 and 4. The check just before publishing (rule 1)
and rule 3 have no test: no test can place a signal between the last row and
publication.

On Windows, Ctrl-C reaches the process as SIGINT and is handled the same
way, except that the process exits with 130 rather than ending by the
signal. Windows does not send SIGTERM. Ctrl-Break and closing the console
end the process without a result. This is reasoned from the dependency's
documentation and is not tested.

## Alternatives

- **Keep rule 7 of ADR-0002.** No dependency, and a supervisor still learns
  from the process that it ended. A consumer that did not supervise the run
  cannot tell a stopped run from a crash.
- **Exit with 130 or 143 instead of ending by the signal.** The same code
  for a shell, but a shell then takes the interrupt as handled by the
  program and carries on: a loop of runs does not stop on Ctrl-C.
- **Clean up and end without a result.** Simpler, and leaves a consumer
  unable to tell a stopped run from a crash.
- **Finish the run and ignore the signal.** Lets a long conversion outlive
  the request to stop it.

## Consequences

- A result with status `cancelled` establishes no verdict when matched, as
  any status other than `ok` does.
- A run normally stops within one row of either read, or after assembling
  the workbook from its scratch files, which checks nothing. A read that
  blocks, such as rows from a pipe whose writer has stalled, holds the run
  until data or the end of input arrives, or until a second signal.
- A signal that arrives while the schema or policy is read takes effect at
  the first row. If either document is refused first, the run ends refused,
  not cancelled.
- A run started with SIGINT ignored, as a non-interactive shell starts a
  background job, still stops on SIGINT: the tool cannot inspect the
  inherited disposition without `unsafe` code.
- A run killed by a signal it does not handle, or ended by a second signal
  before its result is published, still leaves no result, as SecDR-0001
  says of a run killed by a supervisor.
- The lockfile gains `signal-hook` and `signal-hook-registry`.
