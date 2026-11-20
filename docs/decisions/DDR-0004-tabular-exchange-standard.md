---
id: "DDR-0004"
title: "The Tabular Exchange Standard governs the table model and the documents"
status: "proposed"
date: "2026-11-20"
supersedes: "DDR-0002-typed-table-and-document-formats.md"
relates-to:
  - "DDR-0001-exact-values-and-representation.md"
  - "ADR-0002-conversion-result-and-process-boundary.md"
  - "PDR-0002-decision-records-and-upstream-references.md"
  - "../xlsx-write.md"
---

# DDR-0004: The Tabular Exchange Standard Governs the Table Model and the Documents

## Context

[DDR-0002](DDR-0002-typed-table-and-document-formats.md) defined the typed
table model and the schema, policy and result documents in this repository,
called their formats provisional, and intended the portable parts for a
shared contract. That contract is published: the
[Tabular Exchange Standard](https://github.com/fulmenhq/crucible/blob/ced5f8ed51ecf52e7add68986d0486201c5921ea/docs/standards/data/tabular-exchange.md),
contract revision `tabular-exchange/0.1.0`, recorded upstream in
[ADR-0015](https://github.com/fulmenhq/crucible/blob/ced5f8ed51ecf52e7add68986d0486201c5921ea/docs/architecture/decisions/ADR-0015-tabular-exchange-contract.md).
The upstream record has status experimental; following it is a choice made
here.

## Decision

1. **The standard governs what it defines.** The table model, the closed
   list of types, the value grammar, the rows encoding, the schema, policy
   and result documents, the statuses, categories and registered rule
   identifiers, and the procedure for matching a result to an output and to
   a request are those of `tabular-exchange/0.1.0`. This repository does
   not restate them as decisions of its own. Where an earlier record here
   and the standard state the same rule, the standard is the authority.
2. **The documents carry the standard's identifiers.** They are
   `tabular-exchange.schema/0`, `tabular-exchange.policy/0` and
   `tabular-exchange.result/0`, and a result names the contract revision.
   The identifiers used before adoption are other formats: they are refused,
   not read as aliases, and no build reads both.
3. **One contract revision is implemented at a time.** A build names the
   revision it implements and is changed to move to another. The revision of
   a document's format, of the contract and of an adapter profile stay
   separate and are never inferred from one another.
4. **What this program adds is in one extension namespace.** Settings of the
   workbook destination and limits in a policy, and the tool, backend, limits
   in effect and resolved-policy digest in a result, are members of the
   `tableprims` namespace under `extensions`. A policy carrying any other
   namespace is refused. The content written to a result is a fixed set of
   members inside the bounds the standard sets.
5. **A policy that is complete under the standard can still be refused.** A
   workbook needs a worksheet name, and the standard has no member for one.
   A policy that names none is a valid document and is refused when it is
   judged against the adapter profile, with a rule identifier of this
   program. No default name is supplied.
6. **Rules of this program do not reuse registered identifiers.** An
   identifier defined here names a rule of the adapter profile, the
   destination or the file system, and is documented in
   [the command's documentation](../xlsx-write.md).
7. **A workbook carries its own metadata.** The XLSX write profile records,
   in a hidden worksheet named `_tableprims`, each column's identifier,
   type, precision, scale, nullability and representation, and the rows
   holding an empty string. A blank data cell is a null unless that worksheet
   lists it. The name is reserved in any letter case. There is no switch to
   omit the worksheet.
8. **Metadata read from a file is untrusted.** When workbooks are read, the
   metadata is validated in full before it is interpreted, and unknown or
   contradictory metadata is refused. It never overrides a limit or anything
   the caller specified.
9. **Mechanics stay here.** Workbook layout, command-line flags, backend
   pins, exit-code mapping, file publication and the definition of the
   adapter profile are decided in this repository.
10. **Conformance is shown with the standard's vectors.** The vectors and
    fixtures are taken from an upstream release at a fixed revision and run
    against this implementation. Until they are, conformance is not claimed
    beyond what the documentation states was run.

## Alternatives

- **Keep the local formats and translate at the edge.** Two descriptions of
  the same documents, and a consumer could not test this program against the
  shared vectors.
- **Read the earlier identifiers as aliases.** The earlier documents differ
  in shape, so an alias would accept a document under rules it was not
  written for.
- **Supply a default worksheet name.** Every policy valid under the standard
  would run, and a destination setting the author did not state would be
  chosen for them.
- **Edit DDR-0002 to point upstream.** The record of what was decided before
  adoption would be lost.

## Consequences

- Documents written for earlier builds are refused until they are rewritten;
  the change of shape is described in the command's documentation.
- A resolved-policy digest computed by an earlier build does not equal one
  computed now for the same settings.
- A change of contract revision upstream reaches this program only when a
  build adopts it.
- Rules 7 and 8 are carried forward from DDR-0002 unchanged.
- The standard's result-reading and matching rules are not exercised by this
  program, which writes results and does not read them.
