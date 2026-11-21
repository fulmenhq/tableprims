# tableprims-xlsx

XLSX write adapter for the `tableprims` library. It writes a typed table to
one worksheet and records types, scale and empty strings on a hidden
`_tableprims` worksheet.

The worksheet name and the limits of a write come from the `tableprims`
extension namespace of a `tabular-exchange.policy/0` document. A policy that
names no worksheet is refused with `unsupported-representation`,
`sheet-name-missing`, and one that names `_tableprims` in any letter case
with `sheet-name-reserved`. Refusals of rows and values carry the categories and
rule identifiers of the Tabular Exchange Standard, contract revision
`tabular-exchange/0.1.0`, published in
[fulmenhq/crucible](https://github.com/fulmenhq/crucible); rules of this
profile, such as `sheet-name-unsupported` and `too-many-rows`, are its own.

The workbook layout is provisional and may change before the first release.

See the [repository README](https://github.com/fulmenhq/tableprims) for status.
