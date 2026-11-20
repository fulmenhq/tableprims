---
id: "ADR-0003"
title: "SIGINT and SIGTERM stop a run, which says so in its result"
status: "proposed"
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
   handlers for both before it does anything else. A handler only sets a
   flag. The flag is checked before every row of both reads of the rows and
   once more after the workbook is written and synced, before it is
   published.
2. **A stopped run publishes a result that says so.** The status is
   `cancelled`, the refusal is category `cancelled`, rule `cancelled`, with
   no row or column. The inputs read to their end are bound as in any other
   result. No workbook is published, and this program's temporary files and
   the workbook writer's scratch files are removed. The exit code is 130
   after SIGINT and 143 after SIGTERM.
3. **Once the workbook is published, the run completes.** A signal that
   arrives after the last check does not stop the run: it publishes its
   result and exits 0.
4. **A second SIGINT or SIGTERM ends the process at once**, with the exit
   code of that signal and without a result. This leaves the state that
   [DDR-0003](DDR-0003-publishing-output-files.md) describes for a run that
   is killed: no partial file under a published name, possibly temporary
   files, and possibly a workbook without a result.
5. **Other signals keep their default action.** SIGKILL cannot be handled.
   SIGHUP, SIGQUIT and SIGPIPE are not handled.
6. **The handler is a flag-only dependency of the tool.** `signal-hook`
   with its default features off, in `tableprims-cli` only. The library and
   the adapter keep taking the flag as an input and gain no dependency.

## Platforms

On Unix the behaviour above is tested. On Windows, Ctrl-C reaches the
process as SIGINT and is handled the same way. Windows does not send SIGTERM.
Ctrl-Break and closing the console end the process without a result. A
second Ctrl-C can reach the default handler and end the process at once.
This is reasoned from the dependency's documentation and is not tested.

## Alternatives

- **Keep rule 7 of ADR-0002.** No dependency, and a supervisor still learns
  from the process that it ended. A consumer that did not supervise the run
  cannot tell a stopped run from a crash.
- **Clean up and exit without a result.** Simpler, and leaves the same
  ambiguity for a consumer.
- **Finish the run and ignore the signal.** Lets a long conversion outlive
  the request to stop it.

## Consequences

- A result with status `cancelled` establishes no verdict when matched, as
  any status other than `ok` does.
- How long a run takes to stop is bounded by one row of either read, or by
  assembling the workbook from its scratch files, which checks nothing.
- The lockfile gains `signal-hook` and `signal-hook-registry`.
- Tests on Unix send SIGINT and SIGTERM while the rows are read and while the
  workbook is written, and a second signal, and check the exit code, the
  result and what is left on disk; see
  `crates/tableprims-cli/tests/xlsx_write.rs`.
