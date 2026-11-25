// SPDX-License-Identifier: MIT OR Apache-2.0

package table

import "strconv"

// Capability identifiers. Each names an operation of the tableprims library
// in either of its implementations; [Capabilities] lists the ones this
// package implements.
const (
	// CapabilityReadSchema reads a schema document: [ReadSchema].
	CapabilityReadSchema = "read-schema"
	// CapabilityReadPolicy reads a policy document: [ReadPolicy].
	CapabilityReadPolicy = "read-policy"
	// CapabilityReadRows reads and checks rows under a plan: [NewRowReader].
	CapabilityReadRows = "read-rows"
	// CapabilityQuantize quantizes a decimal: [Decimal.Quantize].
	CapabilityQuantize = "quantize"
	// CapabilityBinary64Decision exposes the nearest binary64 value of an
	// exact number, its exactness and its shortest-spelling equality
	// ([Integer.Binary64], [Decimal.Binary64]), plus eligibility under a
	// named policy, today xlsx-write/r1 ([XLSXWriteR1Binary64FromInteger],
	// [XLSXWriteR1Binary64FromDecimal]). It grants no workbook capability.
	CapabilityBinary64Decision = "binary64-decision"
	// CapabilityXLSXWrite writes an XLSX workbook. The Rust XLSX adapter
	// implements it; this package does not.
	CapabilityXLSXWrite = "xlsx-write"
)

// Capability dispositions, as returned by [CapabilityError.Disposition].
const (
	// DispositionUnsupported is a known capability this package does not
	// implement.
	DispositionUnsupported = "unsupported"
	// DispositionUnknown is an identifier that names no capability.
	DispositionUnknown = "unknown"
)

// registry lists every known capability, in a fixed order, and whether this
// package implements it. It is the only source for Capabilities and Require.
var registry = [...]struct {
	id   string
	here bool
}{
	{CapabilityReadSchema, true},
	{CapabilityReadPolicy, true},
	{CapabilityReadRows, true},
	{CapabilityQuantize, true},
	{CapabilityBinary64Decision, true},
	{CapabilityXLSXWrite, false},
}

// Capabilities returns the identifiers of the capabilities this package
// implements, in a fixed order. The list is fixed at build time; the caller
// owns the returned slice.
func Capabilities() []string {
	var out []string
	for _, entry := range registry {
		if entry.here {
			out = append(out, entry.id)
		}
	}
	return out
}

// Require reports whether this package implements a capability. It returns
// nil for one it implements and a *CapabilityError otherwise. Call it before
// selecting an operation.
//
// A capability error is not a [Refusal]: no document, value or row has been
// read, and its outcome is not one of the categories of the Tabular
// Exchange Standard.
func Require(capability string) error {
	for _, entry := range registry {
		if entry.id == capability {
			if entry.here {
				return nil
			}
			return &CapabilityError{capability: capability, disposition: DispositionUnsupported}
		}
	}
	return &CapabilityError{capability: capability, disposition: DispositionUnknown}
}

// CapabilityError is the error [Require] returns for a capability this
// package does not implement or does not know.
type CapabilityError struct {
	capability  string
	disposition string
}

// Capability returns the identifier that was required.
func (e *CapabilityError) Capability() string { return e.capability }

// Disposition returns [DispositionUnsupported] or [DispositionUnknown].
func (e *CapabilityError) Disposition() string { return e.disposition }

// Error returns "capability-unsupported" or "capability-unknown" and the
// quoted identifier.
func (e *CapabilityError) Error() string {
	return "capability-" + e.disposition + ": " + strconv.Quote(e.capability)
}
