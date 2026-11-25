# Exact numbers as binary64 values

Some destinations hold a number only as an IEEE 754 binary64 value: a
spreadsheet number cell, or a DOUBLE column in a columnar format or a
database. tableprims keeps integers and decimals exact. This page describes
what both implementations say about an exact number as a binary64 value,
and the rule the XLSX writer applies. This interface was added in v0.1.1.

## Facts

For an integer or a decimal, both implementations give the nearest binary64
value, ties to even, and two facts about it:

- **exact**: the binary64 value is the number itself;
- **round-trips**: the shortest decimal spelling of the binary64 value
  denotes the number.

The shortest spelling has the fewest significant digits that read back as
the binary64 value and, among those, is the closest to it. When two are
equally close, it is the one whose last digit is even. Go's
`strconv.FormatFloat` and Python's `repr` print this spelling. Rust's
`f64` `Display` breaks such a tie the other way, so the Rust library
resolves it itself. For example, the binary64 value nearest to
`1657035451829198.2` lies exactly halfway between it and
`1657035451829198.3`. The first round-trips and the second does not.

The two are independent:

| Number                         | Exact | Round-trips | Why                                                                    |
| ------------------------------ | ----- | ----------- | ---------------------------------------------------------------------- |
| decimal `0.50`                 | yes   | yes         | one half is a binary fraction                                          |
| decimal `0.1`                  | no    | yes         | no binary fraction is one tenth, but `0.1` spells the nearest one      |
| integer `1152921504606846976`  | yes   | no          | 2^60 is a binary64 value, whose shortest spelling is `1152921504606847000` |
| integer `9007199254740993`     | no    | no          | 2^53 + 1 rounds to 2^53                                                |

Which facts a destination requires, and any limit on the number of digits,
is that destination's rule. A reader that keeps the shortest spelling can
recover a number that round-trips; a reader that keeps the binary value can
recover one that is exact.

The value is finite and zero is positive. Exactness is decided in integer
arithmetic: a decimal is exact when it is a binary fraction whose odd part
fits in 53 bits. The round-trip fact uses the language's correctly rounded
parsing and shortest formatting of binary64 values, with a tie resolved in
integer arithmetic. The comparison with the number is made on digit
strings.

| Surface    | Facts                                                     | XLSX rule                                                                   |
| ---------- | --------------------------------------------------------- | --------------------------------------------------------------------------- |
| Go `table` | `Integer.Binary64()`, `Decimal.Binary64()`: `Value`, `Exact`, `RoundTrips` | `table.XLSXWriteR1Binary64FromInteger`, `table.XLSXWriteR1Binary64FromDecimal`, `*table.Binary64Error` |
| Rust       | `binary64::integer_facts`, `binary64::decimal_facts`: `value`, `is_exact`, `round_trips` | `binary64::from_integer`, `binary64::from_decimal`, `Binary64Error`          |

## The rule of the XLSX write profile

`xlsx-write/r1` stores an exact number as a number cell only when a reader
can recover it from the cell. A value is eligible when, in this order:

1. it has at most 15 significant digits, not counting leading or trailing
   zeros: `too-many-digits` otherwise;
2. it round-trips: `not-round-trip` otherwise;
3. for an integer, it is exact: `not-exact-integer` otherwise.

The first reason that applies is the one reported; both implementations
give the same reason codes. `not-round-trip` cannot occur in practice,
because a number of at most 15 significant digits always round-trips. A
decimal need not be exact: `0.1` is eligible. The decimal
`100000000000000000000000.0` is eligible, while the integer `10^23` is not.

The 15 digits are significant digits without trailing zeros. That is not
the number of digits in the coefficient, which is what
`Decimal.SignificantDigits` in Go and `Decimal::significant_digits` in Rust
count: `1.23456789012345000000` has 21 coefficient digits and is eligible.

A value the rule does not admit is not a refusal of the Tabular Exchange
Standard, and not a capability error. The XLSX writer decides what follows
from the policy's `exact_numbers`: it refuses the run at that cell, or
writes the whole column as text ([xlsx-write.md](xlsx-write.md#exact-numbers)).

The capability `binary64-decision` exposes the facts and eligibility under
a named policy, today `xlsx-write/r1`, on both implementations. It grants no
workbook capability ([capabilities.md](capabilities.md)).

## The suite

`conformance/binary64/v0/binary64.json` belongs to this repository; it is not
part of the standard. Its 63 cases give, for each number, the bits of the
nearest binary64 value, both facts, the `xlsx-write/r1` verdict
(`xlsx_write_r1`) and, for a
decimal, its coefficient digits. They cover:

- 15, 16 and 17 significant digits, and trailing zeros;
- 2^53 and its neighbours, 2^60 and 2^64;
- ties to even at 2^54;
- two shortest spellings equally close to the binary64 value, at four
  magnitudes, each as a pair: the even spelling round-trips and the odd one
  does not;
- a tie at the power of two 2^-24, where the even spelling reads back as a
  different binary64 value and so is not the shortest spelling;
- decimals with a fraction on either side of the 53-bit limit of the odd
  part;
- 10^22, which is exact, and 10^23, which is not;
- zero at several scales;
- the smallest and largest magnitudes of the exact domain.

No case is subnormal, because no exact number of at most 38 digits is.

The suite also lists the three reason codes of the `xlsx-write/r1` rule
with their error text; both implementations give exactly that text.

`scripts/binary64-vectors.py` derives every expectation with CPython,
independently of both implementations:
- the value from its correctly rounded `float`, checked against
  `float(Fraction)`;
- **exact** from `Fraction`;
- **round-trips** from `repr`.

`make binary64-vectors-check` fails when the file differs from what the
script writes. The Go test (`table/binary64_test.go`) and the Rust test
(`crates/tableprims/tests/binary64.rs`) run every case and require a
disagreement when any expectation is altered.
