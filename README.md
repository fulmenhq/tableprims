# tableprims

Typed tabular conversion library, with a thin command-line tool named
`tablebolt`.

tableprims converts typed tables to and from spreadsheet and columnar formats
under an explicit contract: exact integers and decimals never pass through
binary floating point, strings stay strings, and any change of representation
is selected by the caller and disclosed in the result.

## Status

Pre-release. Nothing is implemented or published yet.
