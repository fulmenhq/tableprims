# Decision records

Records follow the [Crucible decision taxonomy](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/repository/decision-records.md):
`<TYPE>-<NNNN>-<slug>.md`, numbered per type, never reused. Security records
are in [`docs/security/`](../security/); all others are here. A record is
`proposed` until the maintainer accepts it; see
[PDR-0002](PDR-0002-decision-records-and-upstream-references.md).

| ID | Title | Status |
| --- | --- | --- |
| [ADR-0001](ADR-0001-library-first-rust-and-go.md) | A library first, in Rust and then pure Go, under one version | proposed |
| [ADR-0002](ADR-0002-conversion-result-and-process-boundary.md) | The conversion result is the record; process integration stays at the tool | proposed |
| [DDR-0001](DDR-0001-exact-values-and-representation.md) | Exact values, and representation changes only by policy | proposed |
| [DDR-0002](DDR-0002-typed-table-and-document-formats.md) | A typed table model, strict documents, and what belongs in a shared contract | proposed |
| [DDR-0003](DDR-0003-publishing-output-files.md) | Output files are published complete, under names that did not exist | accepted |
| [PDR-0001](PDR-0001-independent-expected-results.md) | Expected results are authored independently of the implementation | proposed |
| [PDR-0002](PDR-0002-decision-records-and-upstream-references.md) | How decisions are recorded and how upstream material is referenced | proposed |
| [SecDR-0001](../security/SecDR-0001-untrusted-input-and-dependencies.md) | Inputs are untrusted and bounded; dependencies are permissive and offline | proposed |
