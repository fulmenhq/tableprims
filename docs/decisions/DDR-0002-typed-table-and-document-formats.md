---
id: "DDR-0002"
title: "A typed table model, strict documents, and what belongs in a shared contract"
status: "accepted"
date: "2026-11-19"
relates-to:
  - "DDR-0001-exact-values-and-representation.md"
  - "ADR-0002-conversion-result-and-process-boundary.md"
  - "../xlsx-write.md"
---

# DDR-0002: A Typed Table Model, Strict Documents, and What Belongs in a Shared Contract

## Context

Spreadsheet and delimited formats carry no reliable types, no difference
between a null and an empty string, and no stable column identity. A typed
conversion needs these stated somewhere both sides can read. The statement
has to be independent of any one file format, and the parts of it that other
projects consume should not be defined by one implementation.

## Decision

1. **A table is a schema and ordered rows.** A column has an identifier,
   unique within the schema, and a label, which is what a person sees and may
   repeat or be empty. A column has a type and states whether it may be null.
   A row has exactly one value per column, by position.
2. **The types are a closed list.** They are `string`, `boolean`, `integer`,
   `decimal` with a precision and scale, `date`, and `local-timestamp` and
   `offset-timestamp` with a number of fractional digits. A type outside the
   list is refused as unsupported. Binary floating point, bytes, time of day,
   named time zones and nested values are not in the list, and nothing is
   flattened or turned into a string to make it fit.
3. **Null, empty and missing are different.** A null is not an empty string.
   A row with too few or too many values is refused; a missing trailing value
   is never read as null.
4. **The model is separate from its encodings.** Rows are newline-delimited
   JSON, one array per line, whose elements are `null`, `true`, `false` or a
   string. The schema is a separate document, not a header line in the row
   stream.
5. **Documents are strict.** The schema, policy and result are each one JSON
   object that names its format and revision in a `format` member. A document
   that is not well-formed JSON, or that repeats a key at any depth, is
   refused as malformed. A reader then checks `format` before any other
   member and refuses a revision it does not support. An unknown member is
   refused at every depth. No member names a path.
6. **Three revisions are kept apart.** The revision of a document's format,
   the revision of any shared contract, and the revision of an adapter's
   local profile are separate and are never inferred from one another. The
   result names the profile and the digest of its definition.
7. **A workbook carries its own metadata.** The XLSX write profile records,
   in a hidden worksheet named `_tableprims`, each column's identifier, type,
   precision, scale, nullability and representation, and the rows holding an
   empty string. A blank data cell is a null unless that worksheet lists it.
   The name is reserved in any letter case. There is no switch to omit the
   worksheet.
8. **Metadata read from a file is untrusted.** When workbooks are read, the
   metadata is validated in full before it is interpreted, and unknown or
   contradictory metadata is refused. It never overrides a limit or anything
   the caller specified.
9. **Portable semantics go to a shared contract; mechanics stay here.** The
   value grammar, the schema model, the names of policies, and the result
   fields another project needs (status, run and input binding, output
   digest and length, and the rule for matching a result to its output) are
   intended for a format-neutral contract published in
   [FulmenHQ Crucible](https://github.com/fulmenhq/crucible). Workbook
   layout, command-line flags, backend pins, exit-code mapping and file
   publication stay in this repository.
10. **Until that contract is published, the formats are provisional.** Each
    carries revision 0 and may change before the first release. Nothing in
    this repository assigns an identifier in the shared contract.

## Alternatives

- **Use Arrow as the public model.** It is a strong typed interchange, but it
  does not express spreadsheet provenance or representation policy, and it
  would put a backend's types in every interface.
- **Put the schema in the first line of the row stream.** One file fewer, and
  a schema that cannot be digested or reused apart from its data.
- **Tolerate unknown members.** It eases evolution and lets a misspelled
  policy member silently do nothing.

## Consequences

- A producer must convert its data to the seven types before conversion;
  floating point columns cannot be written at present.
- A document written for a later revision is refused by an earlier reader
  until that reader is taught it.
- CSV has no place for types or nulls, so a typed CSV conversion will need a
  schema supplied beside the file.
- A workbook whose hidden worksheet is removed or edited by another program
  is no longer a typed table.
