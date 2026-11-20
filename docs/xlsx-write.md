# Writing an XLSX workbook

`tablebolt xlsx write` converts typed rows to one worksheet of a new workbook
and records the outcome in a result file.

The rows, the schema, the policy and the result follow the Tabular Exchange
Standard, contract revision `tabular-exchange/0.1.0`, published in
[fulmenhq/crucible](https://github.com/fulmenhq/crucible). The standard
defines the value grammar, the three documents, the categories and rule
identifiers of a refusal, and how a result is matched to an output and to a
request. This page describes what `tablebolt` adds: the workbook, the
`tableprims` extension namespace, its own rule identifiers, its limits and its
exit codes. The workbook layout and the extension members are provisional and
may change before the first release.

```sh
tablebolt xlsx write --input rows.ndjson --schema schema.json \
    --policy policy.json --output report.xlsx --result result.json \
    [--run-id ID]
```

Nothing is read from stdin or written to stdout. The outcome is the exit code
and the result file. `--output` and `--result` must not exist; an existing
file is never replaced.

## Inputs

**Rows** are UTF-8 newline-delimited JSON without a byte order mark. Each line
is one array with exactly one element per schema column. An element is `null`,
`true`, `false` or a string. Integers, decimals, dates and timestamps are
strings; a bare JSON number is refused.

An element that is a JSON number, such as `7`, `-0`, `1.5` or `1e3`, is
refused with rule `numeric-literal`: quote the value. An element that only
begins like a number, such as `-`, `01`, `1.`, `1e` or `1x`, is refused with
`malformed-row`: the line is not JSON.

| Column type        | String form                                             |
| ------------------ | ------------------------------------------------------- |
| `string`           | any text; `""` is an empty string, distinct from `null` |
| `boolean`          | not a string: `true` or `false`                         |
| `integer`          | `-?(0\|[1-9][0-9]*)`, at most 38 digits, no `-0`         |
| `decimal`          | as integer, with exactly `scale` fractional digits      |
| `date`             | `YYYY-MM-DD`                                            |
| `local-timestamp`  | `YYYY-MM-DDThh:mm:ss` plus exactly `fraction` digits    |
| `offset-timestamp` | as local timestamp, then `Z` or `±hh:mm`                |

A timestamp is first judged as a timestamp of either type, and only then
against its column. Text with a bad calendar or clock, a point that no digit
follows, or trailing text that is not `Z` or an offset in range is
`invalid-input` whatever the column declares (`temporal-malformed`,
`temporal-invalid-date`, `temporal-invalid-time`, `temporal-invalid-offset`,
or `temporal-negative-zero-offset` for `-00:00`). A well-formed timestamp that
is not the one the column declares is `schema-mismatch`:
`temporal-fraction-mismatch` when the number of fraction digits differs, and
otherwise `temporal-offset-mismatch` when a `local-timestamp` column is given
an offset or an `offset-timestamp` column is given none.

**Schema**:

```json
{
  "format": "tabular-exchange.schema/0",
  "columns": [
    {"id": "invoice", "label": "Invoice", "type": "string", "nullable": false},
    {"id": "amount", "label": "Amount", "type": "decimal", "precision": 12, "scale": 2, "nullable": true},
    {"id": "seen", "label": "Seen", "type": "local-timestamp", "fraction": 3, "nullable": true}
  ]
}
```

**Policy**:

```json
{
  "format": "tabular-exchange.policy/0",
  "exact_numbers": "require-exact",
  "quantize": [{"column": "amount", "scale": 2, "rounding": "half-even"}],
  "extensions": {
    "tableprims": {
      "sheet": "Data",
      "limits": {"max_rows": 1048575, "max_line_bytes": 1048576, "max_empty_ranges": 65536}
    }
  }
}
```

`quantize` is optional. Rounding modes are `half-even`,
`half-away-from-zero`, `toward-zero`, `toward-positive` and `toward-negative`.
A target `scale` above 38 is an invalid member (`invalid-input`,
`policy-member-invalid`); one that is at most 38 and above the column's
declared precision does not fit the schema (`schema-mismatch`,
`quantize-scale-too-large`).

Both documents refuse unknown members and repeated keys. A number in either
is written `0` or as a nonzero digit followed by digits; any other spelling
is `schema-malformed` or `policy-malformed`. A number spelled that way and
too large for its member is `schema-member-invalid` or
`policy-member-invalid`, however large it is.

### Policy extensions

The settings of this command are members of the `tableprims` namespace under
`extensions`:

| Member   | Required | Meaning                                                                        |
| -------- | -------- | ------------------------------------------------------------------------------ |
| `sheet`  | yes      | name of the data worksheet; not empty and not `_tableprims` in any letter case |
| `limits` | no       | `max_rows`, `max_line_bytes` and `max_empty_ranges`, each optional             |

- `sheet` or `limits` at the top level of a policy is an unknown member
  (`invalid-input`, `policy-member-unknown`), as is any member of the
  namespace other than these two.
- A policy that carries any other namespace is refused (`invalid-input`,
  `policy-extension-unsupported`). No setting is ignored.
- A policy without `extensions`, without the `tableprims` namespace or
  without `sheet` in it is a complete policy under the standard, and this
  command cannot run it: a workbook needs a worksheet name. It is refused
  after the policy document has been read, with `unsupported-representation`,
  `sheet-name-missing`, and exit code 62.
- The standard bounds `extensions` for every reader, and the bounds are
  judged before any namespace is looked up: at most 8 namespaces, at most 256
  values in total, no string or member name longer than 256 code points and
  no number above 9007199254740991 (each `policy-member-invalid`), and a
  document nested at most 6 deep (`policy-malformed`). A namespace that is
  not lowercase letters and digits in groups joined by single hyphens, or is
  longer than 64 characters, is `policy-member-invalid`; a namespace whose
  value is not an object is `policy-not-an-object`.

### Documents written for earlier builds

Before it adopted the standard, this program read `tableprims.schema/0` and
`tableprims.policy/0` and wrote `tableprims.result/0`. Those are different
formats and are not read: a schema or policy that carries one is refused with
`schema-format-unsupported` or `policy-format-unsupported`. To move a
document:

- schema: change `format` to `tabular-exchange.schema/0`;
- policy: change `format` to `tabular-exchange.policy/0` and move `sheet` and
  `limits` from the top level into `extensions.tableprims`;
- result: results are not rewritten. A reader of `tabular-exchange.result/0`
  finds the source digest under `inputs.source` (formerly `inputs.rows`) and
  `tool`, `backend`, `limits` and `policy_digest` under
  `extensions.tableprims` (formerly at the top level).

## Exact numbers

An integer or decimal is written as a numeric cell only when it has at most
15 significant digits and the number stored in the workbook reads back as the
same value; an integer must also be exact in binary floating point. A decimal
is quantized first when the policy asks for it. `exact_numbers` says what
happens otherwise, and it applies to a whole column:

| Value            | Effect                                                         |
| ---------------- | -------------------------------------------------------------- |
| `require-exact`  | the first value that cannot be a numeric cell refuses the run  |
| `text-if-needed` | a column with such a value is written entirely as text         |
| `text-always`    | every integer and decimal column is written as text            |

A numeric cell takes part in spreadsheet arithmetic as a binary floating
point number. Dates and timestamps are written as text.

## The workbook

The first row holds the column labels. A hidden worksheet named `_tableprims`
records each column's identifier, type, precision, scale, nullability and
representation, and the rows that hold an empty string. A blank data cell is a
null unless that worksheet lists it as an empty string. The number format of a
numeric column shows the declared scale; the hidden worksheet is the authority
for it.

## The result file

A run is complete only when the result file exists, its `status` is `ok` and
its `output` digest and length match the workbook; see
[Matching a result](#matching-a-result) for what that does and does not
establish. The workbook is published
first and the result file second, so a workbook without a matching result is
incomplete, whatever is at the `--output` name. The result file is opened
before the conversion starts; if it still cannot be published afterwards, the
run fails and says that the workbook it published has no result. It does
not remove the workbook.

Neither file replaces an existing one, and each appears under its name only
when complete. The file system holding each must support hard links. This is
tried before anything is converted; where it does not hold, the run fails
with code 54 and, when the result file can be written, the rule
`hard-links-unsupported`. Runs given the same `--output` or
`--result` do not coordinate: at most one completes, all of them can
fail, and a workbook can be left without a result. Give each run its own names. The rules are recorded in
[DDR-0003](decisions/DDR-0003-publishing-output-files.md).

```json
{
  "format": "tabular-exchange.result/0",
  "contract": "tabular-exchange/0.1.0",
  "status": "ok",
  "run_id": "case-0001",
  "profile": {"id": "xlsx-write/r1", "sha256": "…"},
  "inputs": {
    "source": {"sha256": "…", "bytes": 79},
    "schema": {"sha256": "…", "bytes": 335},
    "policy": {"sha256": "…", "bytes": 148}
  },
  "output": {"sha256": "…", "bytes": 6103},
  "counts": {
    "rows": 3,
    "columns": [
      {"id": "invoice", "representation": "text", "null": 0, "empty": 1},
      {"id": "amount", "representation": "native", "null": 1, "empty": 0,
       "numeric": 2, "text_fallback": 0, "text_forced": 0, "quantized": 1, "scale_changed": 2}
    ]
  },
  "extensions": {
    "tableprims": {
      "tool": {"name": "tablebolt", "version": "0.1.0", "library": "0.1.0"},
      "backend": {"name": "rust_xlsxwriter", "version": "0.102.0"},
      "limits": {"max_rows": 1048575, "max_line_bytes": 1048576, "max_empty_ranges": 65536},
      "policy_digest": "…"
    }
  }
}
```

- `format` is the wire format of the result and `contract` the revision of
  the standard it was written to. Every member outside `extensions` is
  defined by the standard and can be read without knowing this program.
- `status` is `ok`, `refused`, `failed` or `cancelled`. `output` and `counts` are present
  only for `ok`. Any other status has
  `"refusal": {"category": "…", "rule": "…", "row": 12, "column": "amount"}`,
  with `row` (zero-based) and `column` where they apply. Cell content never
  appears.
- `run_id` is the `--run-id` value, 1 to 128 characters of `[A-Za-z0-9._-]`,
  or `auto-` followed by 32 hexadecimal digits from the system's random
  source.
- Digests are lowercase hexadecimal SHA-256 of the file's bytes. An input
  appears under `inputs` only when it was read to its end: `source` is the
  `--input` file, `schema` and `policy` the two documents. `inputs.policy` is
  the digest of the policy file exactly as supplied.
- `representation` is `native`, `text-fallback`, `text-forced` or `text`. The
  five counters after `empty` appear on integer and decimal columns only.
  `quantized` counts cells whose value changed and `scale_changed` cells whose
  scale changed.
- `profile.sha256` is the digest of the profile definition: the writer and
  its version, the limits and the layout of the hidden worksheet.
- `extensions.tableprims` holds what this program records beyond the
  standard. `tool` and `backend` are always present. `limits`, the limits in
  effect, and `policy_digest` are present once the policy has been read and
  checked against the schema and the profile. A reader that does not know
  the namespace ignores it; deleting `extensions` changes nothing the
  standard defines. The namespace holds 13 values nested to depth 4 of the
  document, inside the bounds the standard sets on `extensions` (8
  namespaces, 256 values, strings of 256 code points, depth 6).
- `policy_digest` is the SHA-256 of the
  [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonical form of the
  resolved policy. That object is this program's own and is not a document
  of the standard. It has the shape of a policy, with every default filled
  in, and adds the profile: `format` (`tableprims.resolved-policy/0`),
  `exact_numbers`, `quantize` (entries in schema column order),
  `extensions.tableprims.sheet`, `extensions.tableprims.limits` (all three,
  defaults filled in) and `profile`. Two policy files that resolve to the
  same policy have the same digest. Expected canonical forms and digests are
  in
  [`policy-digest.json`](../crates/tableprims-cli/tests/fixtures/policy-digest.json).

### Matching a result

`tablebolt` writes results and does not read them. A consumer that holds a
result and a candidate workbook matches them as the standard describes, and
reaches one of two verdicts:

- `output-agreement`: the consumer supplies nothing beyond the result and
  the workbook. The result is valid, says `ok`, and its `output` length and
  digest are the workbook's.
- `requested-run-completed`: the consumer also supplies its own record of
  the request, which is the run identifier it passed as `--run-id` and the
  length and SHA-256 of the rows, schema and policy files. The result is
  valid, says `ok`, carries that `run_id` and those lengths and digests under
  `inputs`, and its `output` length and digest are the workbook's.

`output-agreement` says nothing about which request produced the pair. A
result left by an earlier run, beside a workbook whose bytes happen to be the
same, agrees perfectly. A consumer that acts on behalf of a request needs
`requested-run-completed`.

The stronger verdict needs facts the caller recorded when it made the request,
not facts read back from the result. A caller that omits `--run-id` gets a
generated identifier, which it learns only from the result, so it cannot reach
`requested-run-completed`. Pass `--run-id`, do not reuse one for requests that
must be told apart, and record the length and digest of the rows, schema and
policy files before the run. Supplying only some of the four expectations
establishes neither verdict.

A result whose `status` is not `ok` establishes neither verdict, and so does a
workbook with no result.

Rows cut short at the end of a line are well formed rows, so a run on them
completes and its result agrees with its workbook. Only
`requested-run-completed` shows that the file is not the one the caller
recorded. Rows cut before the closing bracket of the last line are refused:
`malformed-row`, or `invalid-utf8` when the cut splits a character. A cut
that removes only whitespace after the bracket changes nothing that is read.

## Telemetry

A run can report its progress to a supervisor through the process-run
contract of 3leaps Crucible, `contract: process-run/v0`
([standard](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/standards/process-run-contract.md),
a draft; the record adopting it upstream is proposed). Only telemetry is
implemented: there is no control socket, key file or control verb.

Telemetry is off unless a runtime directory is given. `--runtime-dir DIR`
names it; without the flag, the environment variable `TABLEBOLT_RUNTIME_DIR`
does; a blank value counts as unset. There is no default directory. When a
configuration file is added, its key will be `runtime_dir`, below the flag
and the environment variable. Telemetry never affects the workbook, the
result file or the exit code, and its presence or absence is no evidence
that a conversion completed: only a matching result is.

With a runtime directory, the run identifier cannot end in `.` (which covers
`.` and `..`; Windows would drop the dot) or be a name Windows reserves for a
device (`CON`, `PRN`, `AUX`, `NUL`, `COM1` to `COM9`, `LPT1` to `LPT9`, in
any case, with or without an extension); such a run ends with code 64 before
anything is written.

| File                          | Content                                                          |
| ----------------------------- | ---------------------------------------------------------------- |
| `DIR/proc/<run_id>/card.json` | the card: identity, producer and the absolute path of the stream |
| `DIR/events/<run_id>.ndjson`  | the event stream, one JSON object per line                       |

The card is checked against the closed subset of the contract's card
schema that this program writes (no control channel, an absolute stream
path, a UTC start time) before anything of the run is created. It is
published under its name only after `started` is in the stream, and
removed when the run ends, including a run stopped by a signal. A run
killed outright leaves it behind; the contract calls such a card stale. The
stream is kept. `started_at` on the card is the time the run recorded as
it began, shortly after the process started, not the operating system's
start time; compare the two with a tolerance.

Directories this program creates are owner-only (0700) and files owner
read-write (0600) on Unix; an existing runtime directory, `proc` or
`events` directory is used as it is. On Windows the files inherit the
runtime directory's permissions and are not owner-only. The runtime
directory is trusted, as the destination is. A stream is created only if no
file has its name, and a card directory only if none exists for the run
identifier; otherwise what is there is left untouched and the run has no
telemetry. If the card cannot be published, the stream this run began is
removed.

Telemetry fails open: if a directory or file cannot be created or written,
the run continues without it and says nothing. Writing happens on a
separate thread, and the end of a run waits at most two seconds for the
last event. A file system that blocks that thread outright, such as a hung
network mount, can still delay the exit of the process.

### Profile `tableprims.xlsx-write-run/v0`

Every event carries the contract's envelope (`ts` in UTC with milliseconds,
`event`, `run_id`, `seq` from 0, `severity`, `pid`); `started` also carries
the producer. The `data` members are:

| Event       | Severity | `data`                                                                                                                                                        |
| ----------- | -------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `started`   | INFO     | `profile_id` (`xlsx-write/r1`); `source_bytes`, the rows file's length, when it is a regular file                                                             |
| `progress`  | INFO     | `phase` (`scan` or `write`, the first or second read of the rows), `done` and, when known, `total`, in `unit` `bytes`; at most once a second while bytes move |
| `heartbeat` | INFO     | `phase` and `done`; after 5 seconds with no other event                                                                                                       |
| `completed` | INFO     | `status` `ok`, `rows`, and the workbook's `output` digest and length                                                                                          |
| `canceled`  | WARN     | `status` `cancelled`, `reason` `SIGINT` or `SIGTERM`                                                                                                          |
| `failed`    | ERROR    | `status` `refused` or `failed`, `reason` `<category>/<rule>`                                                                                                  |

The last event is the terminal one, written after the result file. It is
`completed` or `canceled` only when a result with status `ok` or
`cancelled` was published; a run whose result could not be published ends
with `failed` and reason `io-failure/result-write-failed`. A run killed
outright, or ended by a second signal, leaves a stream with no terminal
event. No event carries cell content or a path; the card holds the one
path, to the stream. Terminal events carry no `data.artifacts`: there is no
artifact descriptor to point at.

## Vectors of the standard

The vectors, fixtures and JSON Schemas of contract revision
`tabular-exchange/0.1.0` are vendored unedited from the fulmenhq/crucible
release `v0.5.1` under
[`third_party/crucible/`](../third_party/crucible/PROVENANCE.md), with the
origin, commit, licence and a SHA-256 for every file. The tests check those
digests, run the vectors through the built `tablebolt` binary and compare the
status, category, rule and location of each outcome with the one the vector
states. `make check` runs them; nothing is fetched.

| Suite                              | In the release | Run | Agree |
| ---------------------------------- | -------------- | --- | ----- |
| value vectors: `integer`           | 43             | 43  | 43    |
| value vectors: `decimal`           | 45             | 45  | 45    |
| value vectors: `date`              | 28             | 28  | 28    |
| value vectors: `local-timestamp`   | 45             | 45  | 45    |
| value vectors: `offset-timestamp`  | 39             | 39  | 39    |
| value vectors: `cells`             | 25             | 25  | 25    |
| value vectors: `quantize`          | 77             | 77  | 77    |
| row vectors                        | 76             | 76  | 76    |
| document vectors                   | 31             | 31  | 31    |
| fixtures: table schema             | 36             | 36  | 36    |
| fixtures: conversion policy        | 32             | 32  | 32    |
| fixtures: conversion result        | 92             | 0   |       |
| matching vectors                   | 70             | 0   |       |
| total                              | 639            | 477 | 477   |

- **Not run.** `tablebolt` writes results and does not read them, so the 92
  conversion result fixtures and the 70 matching vectors are not run. This
  program has not been judged as a reader of results or as a matcher, and it
  does not validate a document against the JSON Schemas.
- **Quantize vectors.** The binary is compared on the verdict, the rule and
  the `quantized` and `scale_changed` counts. The text of the 75 quantized
  values is read back from the workbook with an independent reader in the
  tests of the workbook writer, which run the same 77 vectors through the
  library.
- **Policy fixtures.** No policy the standard accepts names a worksheet, and
  this command requires one. Each of the 32 fixtures is run as released: a
  fixture the standard refuses must be refused under the stated category and
  rule, and the four it accepts must be read and then refused with
  `sheet-name-missing` and nothing else. The 22 fixtures that are a JSON
  object without an `extensions` member are run a second time with
  `extensions.tableprims.sheet` added, and must then have exactly the stated
  outcome; the four valid ones complete. The other ten carry their own
  `extensions` member or are not an object, and are run as released only.
- **Results written in these runs.** Each of them is checked against what
  the test holds: the members and closed lists of the standard, which members
  go with which status, the registered category of the rule, the run
  identifier, the length and digest of each input and of the workbook, and
  the arithmetic of the counts. That is a check of this program's own output
  and not a reader of results.
- **A check that can fail.** For every suite a test alters or removes an
  expectation, or alters the input, and requires a disagreement. Others
  change, remove and add a vendored file, add a vector no suite counts, and
  alter a written result, and require each to be reported.

The standard checks a reader by running the vectors and comparing category,
rule and location, and checks a consumer of results by running the result
fixtures and the matching vectors. This program is run here as a reader of
rows, schemas and policies only. Nothing in this repository executes the
result fixtures or the matching vectors, and no conformance is claimed beyond
the counts above.

## Exit codes

| Code | Meaning                                                             | `status`    |
| ---- | ------------------------------------------------------------------- | ----------- |
| 0    | complete                                                            | `ok`        |
| 1    | internal failure                                                    | `failed`    |
| 33   | the system ran out of storage or memory                             | `failed`    |
| 50   | permission denied                                                   | `failed`    |
| 51   | an input file does not exist                                        | `failed`    |
| 53   | an input could not be read, or changed while it was read            | `failed`    |
| 54   | a file could not be written, or `--output` or `--result` exists     | `failed`    |
| 60   | a value does not fit the schema, or a configured limit was exceeded | `refused`   |
| 61   | malformed rows, schema or policy                                    | `refused`   |
| 62   | a type, value or sheet name the workbook cannot represent; no sheet | `refused`   |
| 64   | command-line usage error; no result file                            |             |
| 130  | stopped by SIGINT; on Unix the process ends by SIGINT               | `cancelled` |
| 143  | stopped by SIGTERM; on Unix the process ends by SIGTERM             | `cancelled` |

A result file is written for every code except 64, when it can be.

The first SIGINT or SIGTERM stops the run before its next row, or after the
workbook is written and just before it is published. The run then removes
its temporary files and publishes a result with status `cancelled` and
category and rule `cancelled`. Once that result is published, the process
ends by that signal on Unix, which a shell reports as 130 or 143, so a loop
of runs stops; on Windows it exits 130. If the result cannot be published,
the run fails as any run does whose result cannot be written (54 when its
name is taken) and does not end by the signal. A signal that arrives after
that last check does not stop the run: the workbook and result are
published as in any run. A second SIGINT or SIGTERM ends the process at once by
the signal's default action, publishing no result if it has not yet. A read
that blocks, such as rows from a stalled pipe, delays the stop until data or
the end of input arrives, or until a second signal. A supervisor that
sends SIGTERM once and then waits, such as `timeout` without `-k`, can wait
for ever on rows from a stalled pipe: follow SIGTERM with a second signal or
with SIGKILL after a grace period. On Windows, Ctrl-C is handled as SIGINT;
Windows sends no SIGTERM, and Ctrl-Break or closing the console ends the
process without a result. The rules are in
[ADR-0003](decisions/ADR-0003-signals-stop-a-run.md).

A kill, a second signal, or a signal that is not handled ends the process
where it stands. No partly written file is left under the `--output` or
`--result` name, because each is published only when complete; ending
between the two leaves a workbook without a result. Temporary files can
remain beside either and may be deleted: this program's are named
`.tablebolt-<32 hexadecimal digits>.` followed by a purpose and `.tmp`, and
the workbook writer's scratch files `.tmp` followed by six characters.

## Limits

A worksheet holds 1,048,575 data rows and 16,384 columns; a cell holds 32,767
UTF-16 code units. A line of input may be 1 MiB unless the policy raises it,
up to 16 MiB. Empty strings are recorded as at most 65,536 row ranges per
workbook. A schema or policy file may be 16 MiB. A schema nests arrays and
objects at most three deep and a policy at most six; deeper nesting is
`schema-malformed` or `policy-malformed`. An integer or decimal has at most 38 significant digits. Exceeding a
limit refuses the run.

No count or length in a result exceeds 9007199254740991 (2^53 - 1). The limits
above keep every count far below it.

Row data is staged in temporary files in the directory of `--output` while
the workbook is written.
