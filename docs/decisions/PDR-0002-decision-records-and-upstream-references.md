---
id: "PDR-0002"
title: "How decisions are recorded and how upstream material is referenced"
status: "proposed"
date: "2026-11-19"
---

# PDR-0002: How Decisions Are Recorded and How Upstream Material Is Referenced

## Context

This repository depends on standards and contracts kept elsewhere, and makes
decisions of its own. Without a rule, local records restate upstream ones and
go stale, links follow a branch that moves, and a record is quietly edited to
match what was built.

## Decision

1. **Records use the shared taxonomy.** The types are ADR, DDR, SecDR, PDR
   and EPR, named `<TYPE>-<NNNN>-<slug>.md` and numbered per type across the
   repository, as set out in the
   [3leaps Crucible catalog](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/repository/decision-records.md).
   A number is never reused.
2. **Records live in one place.** Every record, of every type, is in
   `docs/decisions/`, and the index in `docs/decisions/README.md` lists each
   one.
3. **A record is accepted by the maintainer.** It is `proposed` until the
   maintainer accepts it, and the acceptance is recorded by the change that
   sets its status.
4. **An accepted record is superseded, not rewritten.** A change of decision
   is a new record that names the one it replaces. An accepted record is
   edited only to correct an error of fact or to add a link.
5. **A record says what could have been chosen instead and what follows.**
   Where a rule can be tested, the record says where.
6. **Acceptance is not implementation.** A record's status says that a
   decision was made. Whether it is built, and how it has been verified, is
   stated in the README and the documentation.
7. **Local decisions stay local.** A choice that concerns only this
   repository is recorded here. A contract other projects consume is proposed
   upstream. Upstream records are not copied into this numbering.
8. **Upstream prose is referenced at a fixed revision.** A link to an
   upstream record or standard names a commit, not a branch.
9. **An upstream record keeps its upstream status.** Following a record that
   is proposed upstream is a local choice and is described as one.
10. **A schema needed to validate is vendored whole.** Where validation needs
    an upstream schema or fixture, the complete set of files it refers to is
    committed with its origin, revision, SHA-256 and licence, and is not
    edited. Nothing is fetched from the network during a conversion or a
    test.

## Alternatives

- **Copy the upstream documents.** Everything is in one place, with two
  owners for the same text.
- **Link to the default branch.** Always current, and the meaning of a record
  here can change without a change here.

## Consequences

- Moving to a newer upstream revision is a reviewed change to links or
  vendored files.
- Nothing is vendored at present: no conversion validates against an
  upstream schema.
