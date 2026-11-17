# tableprims-cli

Builds the `tablebolt` binary, a thin command-line consumer of the
[`tableprims`](../tableprims) library. Conversion semantics live in the library;
this package owns argument parsing, output and exit codes.

At the bootstrap stage the binary offers `version` and `capabilities` only.
