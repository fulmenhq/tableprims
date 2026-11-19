---
id: "DDR-0001"
title: "Exact values, and representation changes only by policy"
status: "accepted"
date: "2026-11-19"
relates-to:
  - "DDR-0002-typed-table-and-document-formats.md"
  - "../xlsx-write.md"
---

# DDR-0001: Exact Values, and Representation Changes Only by Policy

## Context

A value has several properties that a conversion can keep or lose
separately: its mathematical value, its declared scale, the way it was
spelled, how a spreadsheet displays it and how the destination stores it. A
spreadsheet stores every number as a binary floating point number, so an
identifier of twenty digits or an amount with four decimal places cannot
always be stored as a number and recovered. A single "lossless" switch cannot
describe which of these properties a conversion kept.

## Decision

1. **Exact values are held and checked exactly.** An integer or decimal is
   held as a signed coefficient and a scale. Parsing, comparison and
   arithmetic on it, including rounding, never go through binary floating
   point. The only binary number is the one stored in a numeric cell, under
   rule 7.
2. **Exact values travel as canonical strings.** An integer is
   `-?(0|[1-9][0-9]*)`. A decimal is the same followed by a point and exactly
   the declared number of fractional digits; at scale zero it has no point
   and is spelled as an integer. There is no negative zero, no exponent and
   no redundant leading zero. A value has at most 38 significant digits. A bare JSON
   number is refused.
3. **For a number, syntax is judged before the schema.** This applies to
   text read for an integer or decimal column; a string column takes any
   text, such as `SKU-A` or `00123`, unchanged. Text that is not a number in
   this grammar is malformed input, whether the column is integer or
   decimal. A well-formed
   number that does not fit the column's declared type, precision or scale is
   a schema mismatch. A well-formed number of more than 38 digits exceeds a
   limit.
4. **Nothing changes representation unless the policy says so.** The policy
   names one of three behaviours for integers and decimals, for the whole
   workbook: `require-exact` refuses the first value that cannot be stored as
   a number and recovered; `text-if-needed` writes a column containing such a
   value entirely as text; `text-always` writes every such column as text.
   The policy is required and has no default.
5. **Rounding is explicit, per column and counted.** A policy may quantize a
   decimal column to a smaller scale with one of five rounding modes. The
   declared precision is kept and never widened: a rounded value that no
   longer fits refuses the run. The result counts the cells whose value
   changed separately from those whose scale changed.
6. **A column has one representation.** It is fixed before the first cell is
   written, by reading the input once to classify it and a second time to
   write it. The second read must see the same bytes. Earlier cells are never
   rewritten and a column is never part numbers and part text.
7. **A number is stored as a number only when it can be recovered.** For the
   XLSX write profile `xlsx-write/r1` that means: at most 15 significant
   digits, the first of them between 10^-38 and 10^37 in magnitude, and the
   shortest decimal spelling of the nearest binary64 number equal to the
   value. An integer must also equal that binary64 number exactly. Zero
   qualifies at any declared scale. The check
   is made per value in exact arithmetic. The workbook writer and its number
   formatting are pinned as part of the profile; changing either is a new
   profile.
8. **Scale is carried by metadata, not by display.** A numeric cell stores
   `123.4500` as `123.45`. The declared scale is recorded with the column in
   the workbook's metadata ([DDR-0002](DDR-0002-typed-table-and-document-formats.md)),
   and a number format shows it for scales up to 30. The metadata is the
   authority.
9. **Text is written as text.** A string is written as a string cell. It is
   never interpreted as a formula, a link or a number, whatever it looks
   like.
10. **Dates and timestamps keep their spelling.** They are written as text in
    their canonical form, with exactly the declared fractional digits. A
    local timestamp is given no offset, and `Z` and `+00:00` stay distinct.
    Values are not ordered or compared across precisions or offsets.
11. **A numeric cell is a binary number to the spreadsheet.** What is
    guaranteed is that the declared value can be recovered from the stored
    one. Arithmetic done on the cell by a spreadsheet application is binary
    and outside the guarantee.
12. **A reader makes no claim it cannot support.** Reading a workbook must not
    report as exact a value that the producing application, or a parsing
    library, had already rounded.

## Alternatives

- **Write everything as text.** Nothing is lost in storage, but no column
  takes part in spreadsheet arithmetic.
- **Write everything as binary floating point.** Simpler, and wrong for long
  identifiers and for decimals that have no exact binary form.
- **Choose per cell.** It keeps more cells numeric and leaves columns that
  are part numbers and part text, which sort and sum incorrectly.

## Consequences

- Three adjacent integers around 2^53 have 16 digits and are not written as
  numbers under this profile; nor is 10^37, which has one significant digit
  but fails the integer check.
- The input is read twice, so it must be a file that can be read again, and
  row data is staged on disk while the workbook is written.
- Every changed representation is visible in the result: each column reports
  how it was written and how many cells were quantized or changed scale.
- Workbooks have been checked by reading them back with an independent
  parser. Behaviour when a spreadsheet application opens and saves one is not
  established.
