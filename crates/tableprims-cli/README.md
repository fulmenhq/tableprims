# tableprims-cli

Builds the `tablebolt` binary, a thin command-line consumer of the
[`tableprims`](../tableprims) library. Conversion semantics live in the library;
this package owns argument parsing, output and exit codes.

The binary offers `version`, `capabilities` and `xlsx write`. The result file
of a conversion, its digests and the exit codes belong to this package.
