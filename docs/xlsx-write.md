# Writing an XLSX workbook

`tablebolt xlsx write` converts typed rows to one worksheet of a new workbook
and records the outcome in a result file. The formats on this page are
provisional and may change before the first release.

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

| Column type        | String form                                             |
| ------------------ | ------------------------------------------------------- |
| `string`           | any text; `""` is an empty string, distinct from `null` |
| `boolean`          | not a string: `true` or `false`                         |
| `integer`          | `-?(0\|[1-9][0-9]*)`, at most 38 digits, no `-0`         |
| `decimal`          | as integer, with exactly `scale` fractional digits      |
| `date`             | `YYYY-MM-DD`                                            |
| `local-timestamp`  | `YYYY-MM-DDThh:mm:ss` plus exactly `fraction` digits    |
| `offset-timestamp` | as local timestamp, then `Z` or `±hh:mm`                |

**Schema**:

```json
{
  "format": "tableprims.schema/0",
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
  "format": "tableprims.policy/0",
  "exact_numbers": "require-exact",
  "quantize": [{"column": "amount", "scale": 2, "rounding": "half-even"}],
  "sheet": "Data",
  "limits": {"max_rows": 1048575, "max_line_bytes": 1048576, "max_empty_ranges": 65536}
}
```

`quantize` and `limits` are optional. Rounding modes are `half-even`,
`half-away-from-zero`, `toward-zero`, `toward-positive` and `toward-negative`.
Both documents refuse unknown members and repeated keys.

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
its `output` digest and length match the workbook. The workbook is published
first and the result file second, so a workbook without a matching result is
incomplete.

```json
{
  "format": "tableprims.result/0",
  "status": "ok",
  "run_id": "case-0001",
  "tool": {"name": "tablebolt", "version": "0.1.0", "library": "0.1.0"},
  "backend": {"name": "rust_xlsxwriter", "version": "0.102.0"},
  "profile": {"id": "xlsx-write/r1", "sha256": "…"},
  "policy_digest": "…",
  "inputs": {
    "rows": {"sha256": "…", "bytes": 79},
    "schema": {"sha256": "…", "bytes": 335},
    "policy": {"sha256": "…", "bytes": 148}
  },
  "limits": {"max_rows": 1048575, "max_line_bytes": 1048576, "max_empty_ranges": 65536},
  "output": {"sha256": "…", "bytes": 6103},
  "counts": {
    "rows": 3,
    "columns": [
      {"id": "invoice", "representation": "text", "null": 0, "empty": 1},
      {"id": "amount", "representation": "native", "null": 1, "empty": 0,
       "numeric": 2, "text_fallback": 0, "text_forced": 0, "quantized": 1, "scale_changed": 2}
    ]
  }
}
```

- `status` is `ok`, `refused` or `failed`; `cancelled` is reserved. `output` and `counts` are present
  only for `ok`. Any other status has
  `"refusal": {"category": "…", "rule": "…", "row": 12, "column": "amount"}`,
  with `row` (zero-based) and `column` where they apply. Cell content never
  appears.
- `run_id` is the `--run-id` value, 1 to 128 characters of `[A-Za-z0-9._-]`,
  or `auto-` followed by 32 hexadecimal digits from the system's random
  source.
- Digests are lowercase hexadecimal SHA-256 of the file's bytes. An input
  appears under `inputs` only when it was read to its end.
- `representation` is `native`, `text-fallback`, `text-forced` or `text`. The
  five counters after `empty` appear on integer and decimal columns only.
  `quantized` counts cells whose value changed and `scale_changed` cells whose
  scale changed.
- `profile.sha256` is the digest of the profile definition: the writer and
  its version, the limits and the layout of the hidden worksheet.
- `policy_digest` is the SHA-256 of the
  [RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonical form of the
  resolved policy: an object with `format` (`tableprims.resolved-policy/0`),
  `exact_numbers`, `quantize` (entries in schema column order), `sheet`,
  `limits` (defaults filled in) and `profile`. Two policy files that resolve
  to the same policy have the same digest. Expected canonical forms and
  digests are in
  [`policy-digest.json`](../crates/tableprims-cli/tests/fixtures/policy-digest.json).

## Exit codes

| Code | Meaning                                                              | `status`  |
| ---- | -------------------------------------------------------------------- | --------- |
| 0    | complete                                                             | `ok`      |
| 1    | internal failure                                                     | `failed`  |
| 33   | the system ran out of storage or memory                              | `failed`  |
| 50   | permission denied                                                    | `failed`  |
| 51   | an input file does not exist                                         | `failed`  |
| 53   | an input could not be read, or changed while it was read             | `failed`  |
| 54   | a file could not be written, or `--output` or `--result` exists      | `failed`  |
| 60   | a value does not fit the schema, or a configured limit was exceeded  | `refused` |
| 61   | malformed rows, schema or policy                                     | `refused` |
| 62   | a type, value or sheet name the workbook cannot represent            | `refused` |
| 64   | command-line usage error; no result file                             |           |

A result file is written for every code except 64, when it can be.

This version installs no signal handler. An interrupt or a kill ends the
process at once: a supervisor sees death by signal, not exit code 130 or 143,
and no result file with status `cancelled` is produced; those codes and that
status are reserved. Nothing is left under the `--output` or `--result` name,
because each is published only when complete. A temporary file named
`.<name>.<32 hexadecimal digits>.tmp` can remain beside either and may be
deleted.

## Limits

A worksheet holds 1,048,575 data rows and 16,384 columns; a cell holds 32,767
UTF-16 code units. A line of input may be 1 MiB unless the policy raises it,
up to 16 MiB. Empty strings are recorded as at most 65,536 row ranges per
workbook. A schema or policy file may be 16 MiB. Exceeding a limit refuses the
run.

Row data is staged in temporary files in the directory of `--output` while
the workbook is written.
