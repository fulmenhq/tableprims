# Release Signing and Verification

This page states the tableprims release trust model
and its trust files. The signing pins and fingerprint anchors are committed
and change only through review.

## Release identity

A release is one signed annotated tag, `vX.Y.Z`, at a commit of `main`. It
releases the Rust crates and the root Go module
(`github.com/fulmenhq/tableprims`) together. The tag tooling refuses a commit
without the root `go.mod`.

Establish the expected GPG primary key, the exact signing subkey, and the
minisign public-blob fingerprint through an independently trusted maintainer
channel before accepting downloaded public keys. A key distributed beside a
signature is not independent authentication.

## Trust files

| File                                     | Contents                                                                                  |
| ---------------------------------------- | ----------------------------------------------------------------------------------------- |
| `docs/security/release-signing-keys.asc` | Public OpenPGP export: the approved primary key and the exact signing subkey, nothing else |
| `docs/security/tableprims-minisign.pub`  | The organization's minisign public key for release checksum manifests                     |
| `config/release/signing-subkey.txt`      | The fingerprint of the one signing subkey tags may use                                    |
| `keys/expected-fingerprints.txt`         | Two lines: the OpenPGP primary fingerprint, then the minisign public-blob SHA-256         |
| `keys/expected-fingerprints.ndjson`      | The checker's fingerprint records for the same two keys                                   |
| `config/release/tagger-identity.txt`     | The exact git tagger identity for release tags                                            |
| `config/release/release-workflow.sha256` | Digests of every reviewed workflow and local action                                       |

A release publishes no asset or checksum manifest, so nothing is signed with
the minisign key; its pin and anchor are committed so that the anchors are
complete if a release ever does.

## Workflows

Every workflow and local action is pinned by digest in
`config/release/release-workflow.sha256`, and every workflow must declare
top-level `permissions: contents: read` with no job granting more. `make
check` fails on any change until the pin is updated, so a workflow change is
always a reviewed change to a trust file. CI holds no signing, release or
registry authority.

## Pins and anchors

The pins are written once by `make release-export-pin`, from the maintainer's
approved keys, and validated before anything is written; existing pins are
never overwritten. `make release-insert-anchors` derives the two fingerprint
anchors from the committed exports with `decernor` 0.1.8 or newer, twice, and
compares the results. `make release-validate-pin` reads the committed pins and
anchors as git blobs and checks them in an isolated keyring. The minisign
fingerprint covers the decoded public blob, not its comment or the whole file.

## Tags

A release is one tag, `vX.Y.Z`, which releases the Rust crates and the root Go
module together. It must be annotated, signed by the committed signing subkey,
tagged by the committed tagger identity, carry the prepared message (subject
`Release X.Y.Z` and the attribution footer), and point at a commit whose root
`go.mod` declares `github.com/fulmenhq/tableprims`. Verification refuses a
release where the tag is missing or points elsewhere.

Tag verification reads committed public data from the tagged commit in an
isolated keyring and requires the exact signing subkey of the approved primary,
which comes from the maintainer environment, never from the tree. The forge's
own "verified" flag is an additional check.
