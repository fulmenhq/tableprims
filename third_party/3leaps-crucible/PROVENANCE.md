# Vendored files from 3leaps/crucible

The directory `v0.2.12/` holds the JSON Schemas and examples of the
process-run contract, `contract: process-run/v0`, as released. The files are
not edited. Each is at the path it has upstream.

| Fact       | Value                                                  |
| ---------- | ------------------------------------------------------ |
| Repository | <https://github.com/3leaps/crucible>                   |
| Tag        | `v0.2.12`                                              |
| Commit     | `3636a79f7ea507bccfa6534655764d7b36c2ea05`             |
| Files      | 10, 20,784 bytes                                       |
| Digests    | [`SHA256SUMS`](SHA256SUMS), one SHA-256 for every file |

## Upstream paths

| Path                      | Content                                                                                    |
| ------------------------- | ------------------------------------------------------------------------------------------ |
| `schemas/process-run/v0/` | `contract.json`, the card, event and control-exchange schemas, `README.md` and `examples/` |
| `LICENSE-CC0`             | the licence text                                                                           |

Every file under `schemas/process-run/v0/` at that commit is here; nothing was
left out and nothing was added. The control-exchange schema and its examples
come with the directory; this program offers no control channel.

## Licence

CC0-1.0. The upstream repository carries two licences. Its
[`NOTICE`](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/NOTICE),
which is not vendored, places standards documentation, JSON Schemas and
examples under the CC0 1.0 Universal Public Domain Dedication
(`LICENSE-CC0`), and code under the MIT License. No upstream code is
vendored. `NOTICE` also states the upstream trademark terms, which the
dedication does not change.

## Not vendored

- The standard itself, which is prose and is referenced at the same commit:
  [`docs/standards/process-run-contract.md`](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/standards/process-run-contract.md).
  It is a draft, and the record adopting it upstream,
  [ADR-0006](https://github.com/3leaps/crucible/blob/3636a79f7ea507bccfa6534655764d7b36c2ea05/docs/decisions/ADR-0006-process-run-contract.md),
  is proposed.

## How the files are used

The tests check every file against `SHA256SUMS`, validate the card and every
event this program writes against the card and event schemas with an
independent validator, and parse the examples as goldens. Nothing is fetched.
The event schema calls the stream append-only; that this program also
creates each stream exclusively is its own rule, stated in
`docs/xlsx-write.md`.
