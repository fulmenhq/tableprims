# tableprims-cli

Builds the `tablebolt` binary, a thin command-line consumer of the
[`tableprims`](../tableprims) library. Conversion semantics live in the library;
this package owns argument parsing, output and exit codes.

The binary offers `version`, `capabilities` and `xlsx write`. The result file
of a conversion, its digests and the exit codes belong to this package.

The result is a `tabular-exchange.result/0` document written to contract
revision `tabular-exchange/0.1.0` of the Tabular Exchange Standard, published
in [fulmenhq/crucible](https://github.com/fulmenhq/crucible). What the binary
records beyond the standard (the tool, the workbook writer, the limits in
effect and a digest of the resolved policy) is under `extensions.tableprims`.
See [docs/xlsx-write.md](../../docs/xlsx-write.md).
