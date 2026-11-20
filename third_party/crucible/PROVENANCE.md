# Vendored files from fulmenhq/crucible

The directory `v0.5.1/` holds the vectors, fixtures and JSON Schemas of the
Tabular Exchange Standard, contract revision `tabular-exchange/0.1.0`, as
released. The files are not edited. Each is at the path it has upstream.

| Fact       | Value                                                    |
| ---------- | -------------------------------------------------------- |
| Repository | <https://github.com/fulmenhq/crucible>                   |
| Tag        | `v0.5.1`                                                 |
| Commit     | `9e3c2062f0ea46c0536efdf14d3b5e75fb3f30d8`               |
| Files      | 173, 522,165 bytes                                       |
| Digests    | [`SHA256SUMS`](SHA256SUMS), one SHA-256 for every file   |

## Upstream paths

| Path                                      | Content                                                                 |
| ----------------------------------------- | ----------------------------------------------------------------------- |
| `config/data/tabular-exchange/v0.1.0/`    | `contract.json`, the four vector files and `fixtures/` with its manifest |
| `schemas/data/tabular-exchange/v0.1.0/`   | four JSON Schemas, `README.md` and `semantic-validation.md`             |
| `LICENSE-CC0`                             | the licence text                                                        |

Every file under the two directories at that commit is here; nothing was left
out and nothing was added.

## Licence

CC0-1.0. The upstream repository carries two licences. Its
[`NOTICE`](https://github.com/fulmenhq/crucible/blob/9e3c2062f0ea46c0536efdf14d3b5e75fb3f30d8/NOTICE),
which is not vendored, places
standards documentation, JSON Schemas, the contents of `config/` and
conformance test vectors under the CC0 1.0 Universal Public Domain
Dedication (`LICENSE-CC0`), and code under the MIT License. No upstream code
is vendored. `NOTICE` also states the upstream trademark terms, which the
dedication does not change.

## Not vendored

- The standard itself. It is prose and is referenced at the same commit:
  [`docs/standards/data/tabular-exchange.md`](https://github.com/fulmenhq/crucible/blob/9e3c2062f0ea46c0536efdf14d3b5e75fb3f30d8/docs/standards/data/tabular-exchange.md).
  The relative link to it in `schemas/data/tabular-exchange/v0.1.0/README.md`
  does not resolve in this tree.
- The upstream reference checker,
  `scripts/tabular-exchange-reference-check.py`. It is a second
  implementation of the rules. The tests here compare this implementation
  with the expectation each vector states and do not need it.

## Verification

`SHA256SUMS` has the form `shasum -a 256` and `sha256sum` write and check,
with paths relative to `v0.5.1/`. The test target `tabular_exchange` of
`tableprims-cli` holds the SHA-256 of `SHA256SUMS`, compares every file with
its listed digest, and fails on a file that differs, is missing or is not
listed. `make check` runs it. `.gitattributes` turns off line-ending
conversion for these paths, so a checkout has the released bytes on every
platform.

## Moving to another release

Replace `v0.5.1/` with the same paths taken from the new tag, regenerate
`SHA256SUMS`, and update this file, the digest and the counts in the tests,
and the documentation in one reviewed change. An expectation is never changed
to make this implementation pass.
