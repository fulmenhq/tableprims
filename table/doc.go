// SPDX-License-Identifier: MIT OR Apache-2.0

// Package table reads the documents, values and rows of the Tabular Exchange
// Standard, contract revision tabular-exchange/0.1.0, published in
// fulmenhq/crucible.
//
// It is the Go implementation of the tableprims library and answers to the
// same conformance vectors as the Rust crate. It uses the standard library
// only and needs no cgo.
//
// A schema document declares the columns of a table and is read with
// [ReadSchema]. A policy document says how exact numbers are represented and
// which decimal columns are quantized, and is read with [ReadPolicy].
// [NewPlan] checks a policy against a schema. A [RowReader] reads rows in
// the rows encoding (newline-delimited JSON arrays of null, true, false and
// strings) and yields each row as typed values checked under the schema,
// with decimal columns quantized as the plan says.
//
// Exact numbers are held as an integer coefficient and, for decimals, a
// declared scale; nothing is read or computed through binary floating point.
// Dates and timestamps are held as validated fields exactly as written; no
// clock, host time zone or time-zone database is consulted.
//
// Every refusal is a [*Refusal], which names the category and the rule
// identifier the standard registers, and the zero-based row and the column
// identifier when the refusal has them. A message never contains document
// or cell content.
//
// Reading performs no file system access and keeps no global state; values
// of this package are safe for concurrent use once built, except a
// [RowReader], which belongs to one goroutine.
package table

// Format identifiers and the contract revision this package reads.
const (
	// ContractRevision is the revision of the Tabular Exchange Standard this
	// package implements.
	ContractRevision = "tabular-exchange/0.1.0"
	// SchemaFormat is the format identifier of the schema document.
	SchemaFormat = "tabular-exchange.schema/0"
	// PolicyFormat is the format identifier of the policy document.
	PolicyFormat = "tabular-exchange.policy/0"
	// ExtensionNamespace is the one extension namespace of a policy that
	// this package implements. A policy carrying any other is refused.
	ExtensionNamespace = "tableprims"
)

// Bounds of the standard and of this reader.
const (
	// MaxDigits is the largest number of significant digits of an exact
	// number, and the largest decimal precision.
	MaxDigits = 38
	// MaxFractionDigits is the largest number of fractional-second digits a
	// timestamp column may declare.
	MaxFractionDigits = 9
	// MaxCount is the largest number a document may carry: 2^53 - 1.
	MaxCount uint64 = 9_007_199_254_740_991
	// MaxExtensionNamespaces is the largest number of namespaces under the
	// extensions member of a policy.
	MaxExtensionNamespaces = 8
	// MaxExtensionValues is the largest number of values under extensions:
	// every member value and every array element, at every depth, the
	// namespace objects included.
	MaxExtensionValues = 256
	// MaxExtensionString is the largest length, in code points, of a string
	// or a member name under extensions.
	MaxExtensionString = 256
	// MaxNamespaceLength is the largest length of a namespace, in
	// characters.
	MaxNamespaceLength = 64
	// MaxDocumentBytes is the largest schema or policy document this reader
	// accepts, in bytes. A larger document is refused with the rule
	// schema-too-large or policy-too-large.
	MaxDocumentBytes = 16 << 20
)

// Nesting bounds of the documents: a schema is an object, an array and an
// object; a policy reaches six levels only under extensions.
const (
	schemaMaxDepth = 3
	policyMaxDepth = 6
)
