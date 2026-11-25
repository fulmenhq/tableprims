#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Write or check conformance/binary64/v0/binary64.json.

The expectations are derived here with CPython alone, independently of the
Rust and Go implementations:

- the nearest binary64 value is float() of the decimal text, which CPython
  rounds correctly, ties to even; it is cross-checked against
  float(Fraction(text));
- `exact` compares Fraction(value) with Fraction(text), in exact arithmetic;
- `round_trips` compares the shortest spelling, repr(value), with the text
  as decimal numbers; repr gives the fewest digits that read back as the
  value and, among those, the closest, ties to an even last digit;
- `xlsx_write_r1` applies the rule of the XLSX write profile xlsx-write/r1 to
  those facts: at most 15 significant digits not counting trailing zeros,
  then `round_trips`, then, for an integer, `exact`.

Usage: binary64-vectors.py [--check]
"""

import json
import struct
import sys
from decimal import Decimal
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "conformance" / "binary64" / "v0" / "binary64.json"
MAX_SIGNIFICANT_DIGITS = 15

# The reason codes of the xlsx-write/r1 rule and the error text of each, the
# same in both implementations.
REASONS = [
    ("too-many-digits", "value has too many significant digits for a numeric cell"),
    ("not-round-trip", "value does not survive storage as a binary number"),
    ("not-exact-integer", "integer is not exactly representable as a binary number"),
]

# (kind, text, precision, scale): precision and scale are None for integers.
# Grouped by the edge each line exercises.
CASES = [
    # zero, signs and small integers
    ("integer", "0", None, None),
    ("integer", "1", None, None),
    ("integer", "-1", None, None),
    ("integer", "42", None, None),
    # 15, 16 and 17 significant digits
    ("integer", "999999999999999", None, None),
    ("integer", "9999999999999999", None, None),
    ("integer", "12345678901234567", None, None),
    # trailing zeros do not count as significant for eligibility
    ("integer", "123456789012344000", None, None),
    ("integer", "1234567890123456000", None, None),
    # around 2^53
    ("integer", "9007199254740991", None, None),
    ("integer", "9007199254740992", None, None),
    ("integer", "9007199254740993", None, None),
    ("integer", "-9007199254740993", None, None),
    # rounding ties at 2^54: halfway values round to the even neighbour
    ("integer", "18014398509481986", None, None),
    ("integer", "18014398509481990", None, None),
    # exact in binary, but the shortest spelling is a different number
    ("integer", "1152921504606846976", None, None),
    ("integer", "18446744073709551616", None, None),
    # powers of ten: 10^22 is exact, 10^23 is not
    ("integer", "10000000000000000000000", None, None),
    ("integer", "100000000000000000000000", None, None),
    ("integer", "-100000000000000000000000", None, None),
    # the edges of the 38-digit domain
    ("integer", "10000000000000000000000000000000000000", None, None),
    ("integer", "99999999999999999999999999999999999999", None, None),
    ("integer", "-99999999999999999999999999999999999999", None, None),
    # zero at several scales
    ("decimal", "0.00", 2, 2),
    ("decimal", "0.00000000000000000000000000000000000000", 38, 38),
    ("decimal", "0", 1, 0),
    # ordinary decimals
    ("decimal", "0.1", 1, 1),
    ("decimal", "0.3", 1, 1),
    ("decimal", "0.50", 2, 2),
    ("decimal", "-12.50", 4, 2),
    ("decimal", "123.45", 5, 2),
    ("decimal", "2.5", 2, 1),
    # exact binary fractions
    ("decimal", "0.125", 3, 3),
    ("decimal", "0.0001220703125", 13, 13),
    ("decimal", "-0.0001220703125", 13, 13),
    # 15, 16 and 17 significant digits
    ("decimal", "0.999999999999999", 15, 15),
    ("decimal", "9.99999999999999", 15, 14),
    ("decimal", "0.1234567890123456", 16, 16),
    ("decimal", "1.2345678901234567", 17, 16),
    # trailing zeros: the coefficient has 21 digits, 15 or 2 are significant
    ("decimal", "1.23456789012345000000", 21, 20),
    ("decimal", "1.50000000000000000000", 21, 20),
    ("decimal", "1.234567890123456000", 19, 18),
    # an integer value at a scale: no exactness requirement for a decimal
    ("decimal", "100000000000000000000000.0", 25, 1),
    ("decimal", "9007199254740993.0", 17, 1),
    ("decimal", "18014398509481986.0", 18, 1),
    ("decimal", "1152921504606846976.0", 20, 1),
    # two shortest spellings equally close to the binary64 value: the one
    # with the even last digit is the shortest spelling, ties to even
    ("decimal", "1657035451829198.2", 17, 1),
    ("decimal", "1657035451829198.3", 17, 1),
    ("decimal", "766111447580660.2", 16, 1),
    ("decimal", "766111447580660.3", 16, 1),
    ("decimal", "115441097943.48438", 17, 5),
    ("decimal", "115441097943.48437", 17, 5),
    ("decimal", "851197258272.2188", 16, 4),
    ("decimal", "851197258272.2187", 16, 4),
    # a tie at a power of two: 2^-24 lies halfway between two 16-digit
    # spellings, but the even one parses to a different binary64 value, so
    # the odd one is the shortest spelling
    ("decimal", "0.00000005960464477539063", 23, 23),
    ("decimal", "-0.00000005960464477539063", 23, 23),
    # a fraction at the 53-bit boundary of the odd part
    ("decimal", "4503599627370495.5", 17, 1),
    ("decimal", "4503599627370496.5", 17, 1),
    # the smallest and largest magnitudes of the domain
    ("decimal", "0.00000000000000000000000000000000000001", 38, 38),
    ("decimal", "-0.00000000000000000000000000000000000001", 38, 38),
    ("decimal", "0.00000000000000000000000000000000000003", 38, 38),
    ("decimal", "9999999999999999999999999999999999999.9", 38, 1),
    ("decimal", "0.99999999999999999999999999999999999999", 38, 38),
]


def stripped_digits(text):
    digits = text.lstrip("-").replace(".", "").lstrip("0").rstrip("0")
    return max(len(digits), 1)


def coefficient_digits(text):
    digits = text.lstrip("-").replace(".", "").lstrip("0")
    return max(len(digits), 1)


def derive(kind, text, precision, scale):
    value = float(text)
    assert value == float(Fraction(text)), text
    if value == 0:
        value = 0.0  # zero is stored positive
    exact = Fraction(value) == Fraction(text)
    round_trips = Decimal(repr(value)) == Decimal(text)
    if stripped_digits(text) > MAX_SIGNIFICANT_DIGITS:  # reasons in REASONS order
        xlsx_write_r1 = "too-many-digits"
    elif not round_trips:
        xlsx_write_r1 = "not-round-trip"
    elif kind == "integer" and not exact:
        xlsx_write_r1 = "not-exact-integer"
    else:
        xlsx_write_r1 = "ok"
    case = {"kind": kind, "text": text}
    if kind == "decimal":
        assert len(text.partition(".")[2]) == scale, text
        assert coefficient_digits(text) <= precision, text
        case.update(precision=precision, scale=scale, coefficient_digits=coefficient_digits(text))
    case.update(
        bits="0x%016x" % struct.unpack(">Q", struct.pack(">d", value))[0],
        exact=exact,
        round_trips=round_trips,
        xlsx_write_r1=xlsx_write_r1,
    )
    return case


def document():
    assert len({(k, t, p, s) for k, t, p, s in CASES}) == len(CASES), "duplicate case"
    cases = [derive(*c) for c in CASES]
    return {
        "suite": "tableprims-binary64/0",
        "description": (
            "Binary64 facts of exact numbers. bits is the nearest binary64 value, ties to even; "
            "exact says it equals the number; round_trips says its shortest decimal spelling "
            "denotes the number; xlsx_write_r1 is the verdict of the xlsx-write/r1 rule. "
            "Derived with CPython by scripts/binary64-vectors.py."
        ),
        "reasons": [{"code": code, "message": message} for code, message in REASONS],
        "cases": cases,
    }


def render():
    return json.dumps(document(), indent=2) + "\n"


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] and not check:
        sys.exit(__doc__)
    text = render()
    if check:
        if not TARGET.is_file() or TARGET.read_text() != text:
            sys.exit(f"error: {TARGET.relative_to(ROOT)} differs from scripts/binary64-vectors.py")
        print(f"[ok] binary64 vectors: {len(CASES)} cases")
        return
    TARGET.parent.mkdir(parents=True, exist_ok=True)
    TARGET.write_text(text)
    print(f"wrote {TARGET.relative_to(ROOT)}: {len(CASES)} cases")


if __name__ == "__main__":
    main()
