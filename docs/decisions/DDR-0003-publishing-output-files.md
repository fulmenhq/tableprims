---
id: "DDR-0003"
title: "Output files are published complete, under names that did not exist"
status: "accepted"
date: "2026-11-19"
relates-to:
  - "ADR-0002-conversion-result-and-process-boundary.md"
  - "../xlsx-write.md"
---

# DDR-0003: Output Files Are Published Complete, Under Names That Did Not Exist

## Context

A conversion writes two files: the converted file and a result file that
describes it. Either can be interrupted part-way, two runs can be given the
same destination, and another program can create or remove a file under a
destination name while a run is under way. A reader of the destination
directory must be able to tell a finished conversion from anything else
without knowing how the run ended.

## Decision

1. **A destination name appears once, complete.** Each output is written to a
   temporary file in the destination's directory, flushed to storage, and then
   given its name in one step. A reader never sees a partly written file under
   a destination name.
2. **An existing name is never replaced.** Publishing uses an operation the
   system refuses when the name exists. A check made earlier in the run is a
   convenience for failing early and is never the guard. There is no second
   way to publish. Whether the file system offers the operation is tried
   with temporary files before anything is converted; where it does not, the
   run fails then and the destination stays absent. The failure has its own
   rule in the result file; when it is the result file's directory that
   cannot link, there is no result file and the program says so on standard
   error.
3. **The result file is the record of completion.** The converted file is
   published first and the result file second. A conversion is complete only
   when the result file exists, reports status `ok`, and its digest and length
   match the converted file. A converted file with no matching result is
   incomplete, whatever its content.
4. **The result file is opened before converting.** Its temporary file is
   created before any input is read, so a destination directory that cannot
   be written stops the run before a converted file exists. This reserves
   nothing: the result name can still be taken, and later writes can still
   fail.
5. **A published file is never removed.** When the converted file is
   published and the result then cannot be, the converted file stays and the
   run reports it as incomplete. By then the name may refer to a file the run
   did not write, and the system offers no removal that is conditional on
   which file a name refers to.
6. **Temporary names belong to one run.** The program's own carry a token
   drawn from the operating system's random source and do not grow with the
   destination's name. A component that writes the converted file may keep
   scratch files of its own, under names it chooses, in the same directory.
   Leftover temporary files of either kind are never read, reused or cleaned
   up by a later run.
7. **Runs are not coordinated.** Runs given the same destination names do not
   wait for or detect each other. Among such runs, while nothing else removes
   or reuses those names, at most one completes, and a result with status
   `ok` describes the converted file beside it. It is possible for no run to
   complete and for a converted file to be left without a result.
8. **The destination directory is trusted.** The rules hold against
   interruption of the process, concurrent runs and unrelated files appearing
   under a name. They do not hold against a party that can replace the
   directory itself or remove and rewrite files in it between two steps.
9. **Complete is not the same as durable.** Each file is flushed to storage
   before it is named, and the directory entry is flushed where the system
   allows. Rules 1 and 3 are about what a reader can see after a process is
   interrupted; what survives a power loss, and in which order the two names
   survive it, depends on the file system and is not promised.
10. **The result file, not the exit status, is the record.** Exit status 0
    and a matching result file agree when a run is left to finish. A consumer
    that did not supervise the run relies on the result file alone.
11. **Rules that a test can break have one.** The tests run the built
    program, including runs that are killed, runs started together that share
    one or both destination names, a name taken while a run is under way and
    a directory that stops accepting names. Flushing to storage and the
    refusal of a file system without hard links are not covered by automated
    tests.

## Consequences

- A consumer checks the result file, never the presence of the converted file
  alone.
- A run that is killed can leave temporary files, and after the converted
  file is published and before the result is, a converted file with no
  result. Rule 3 makes that state recognisable. Removing such a file is the
  caller's decision.
- Callers give each run its own destination names. Sharing them replaces
  nothing but can end with every run failing.
- The destination's file system must support hard links. Destinations on file
  systems without them are refused.
- These rules bind programs that write files. A caller of the library that
  supplies its own output stream publishes nothing and has no obligation
  here.
- A later conversion or a second implementation shows the same behaviour to
  a reader of the directory: no replacement, and completion read from the
  result file. It may use a different system operation to get there.
