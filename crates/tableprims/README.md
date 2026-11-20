# tableprims

Typed tabular conversion library: exact numbers, typed rows and schemas, and
the schema and policy documents. It has no dependencies. Conversions are
implemented by adapter crates, so `conversions()` here returns an empty list.

The value grammar, the rows encoding and the two documents are those of the
Tabular Exchange Standard, contract revision `tabular-exchange/0.1.0`,
published in [fulmenhq/crucible](https://github.com/fulmenhq/crucible). The
readers accept `tabular-exchange.schema/0` and `tabular-exchange.policy/0`
and no other format identifier. A policy's `extensions` member is held to the
standard's bounds, and the one namespace read is `tableprims`, which carries
an optional `sheet` and optional `limits`; any other namespace refuses the
policy.

See the [repository README](https://github.com/fulmenhq/tableprims) for status.
