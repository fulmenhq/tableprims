# Release Checklist

How a maintainer cuts a tableprims release.

A release is one signed annotated tag, `vX.Y.Z`, at a commit of `main`; it
releases the Rust crates and the root Go module together. The signing material
and the trust rules are in [docs/security/README.md](docs/security/README.md).
This checklist ends at the verified tag: it publishes no package and uploads
no release asset.

Signing and tagging are maintainer steps on the maintainer's machine. CI holds
no signing, release or registry authority.

## Prerequisites

- `gpg`, `minisign`, `gh`, `jq` and `python3` (3.11 or newer, with `PyYAML`
  installed; `Makefile` selects the first of `python3`, `python3.14`,
  `python3.13` or `python3.12` on `PATH` with `PyYAML`, or honors an explicit
  `PYTHON`) and the workspace MSRV toolchain. `make doctor` reports each
  tool's version against the recommended minimum in `.goneat/tools.yaml` and
  fails on any tool that is missing, older, or whose version it cannot read.
- decernor 0.1.8 or newer as a regular file at an absolute path.
- limensafe and the maintainer's catalog, which the pre-tag gate requires for
  the public-surface scan (it never runs in CI).
- The ceremony environment, exported in the shell and kept outside this
  repository:

  ```bash
  export TABLEPRIMS_RELEASE_TAG="vX.Y.Z"                   # explicit; never derived
  export TABLEPRIMS_TAG_MESSAGE_DIR='<message directory>'  # outside the repository; named for the tag
  export TABLEPRIMS_DECERNOR_BIN='<decernor>'              # absolute; a regular file, not a symlink
  export TABLEPRIMS_TAGGER_NAME='3 Leaps Infosec Team'     # config/release/tagger-identity.txt
  export TABLEPRIMS_TAGGER_EMAIL=infosec@3leaps.net
  export TABLEPRIMS_PGP_KEY_ID='<signing subkey>!'         # config/release/signing-subkey.txt
  export TABLEPRIMS_GPG_HOMEDIR='<GPG home>'               # outside the repository; not the default home
  export TABLEPRIMS_GPG_SIGNING_FINGERPRINT='<primary>'    # approved primary, from a trusted channel
  export TABLEPRIMS_RELEASE_ANCHOR_FILE='<anchor record>'  # outside the repository; must not exist yet
  export LIMENSAFE_CATALOG_PATH='<catalog>'                # outside the repository
  ```

  `TABLEPRIMS_TAG_MESSAGE_DIR` must already exist as a directory outside the
  repository whose name equals `$TABLEPRIMS_RELEASE_TAG`. A bare `RELEASE_TAG`
  is refused by `make`.

## Scope

- The root Go module, `go.mod` at the repository root declaring
  `github.com/fulmenhq/tableprims`, is required: tag creation and tag
  verification refuse a commit without it.
- The crates set `publish = false`; registry publication is not covered
  here.

## Public pins

The public pins and their fingerprint anchors are committed and change only
through review:

- `docs/security/release-signing-keys.asc`,
  `docs/security/tableprims-minisign.pub` and
  `config/release/signing-subkey.txt` are the pins. `make release-export-pin`
  writes them once, with `TABLEPRIMS_GPG_HOMEDIR`, `TABLEPRIMS_PGP_KEY_ID`,
  `TABLEPRIMS_MINISIGN_PUB_EXPORT` (the approved minisign public key) and
  `TABLEPRIMS_DECERNOR_BIN` loaded; it refuses to overwrite and writes nothing
  unless all three validate.
- `keys/expected-fingerprints.{txt,ndjson}` are the anchors.
  `make release-insert-anchors` derives them from the committed exports with
  decernor, twice, and compares; it refuses to overwrite.
- [ ] `make release-validate-pin`. The anchors must match the organization's
  published release fingerprints; a mismatch is a stop.

## Changing a workflow

Every file under `.github/workflows/` and `.github/actions/` is pinned by
digest in `config/release/release-workflow.sha256`; every workflow must declare
top-level `permissions: contents: read` with no job granting more. (Any
workflow can be edited into one that runs on a tag, so all of them are
pinned.) Editing one fails `make check` until the pin is updated, and tag
verification checks the pin in the tagged commit. To change one:

- [ ] Edit the workflow or action and update its line in the pin file in the
  same change (`shasum -a 256 <file>`).
- [ ] Have the change reviewed as a trust-file change: the pin moving is the
  signal that CI or release behavior changed.

## 1. Release pack

- [ ] All in-scope PRs merged; on `main`, clean, in sync with `origin/main`.
- [ ] Set the version through the Makefile, never by hand: `make version-patch`,
  `make version-minor`, `make version-major` or `make version-set` with
  `V=X.Y.Z`. `VERSION` is the single source of truth; the target writes it to
  `Cargo.toml` and `Cargo.lock`, and `make version-check` confirms every site
  agrees.
- [ ] `CHANGELOG.md`: move `[Unreleased]` to `## [X.Y.Z] - YYYY-MM-DD`, and
  **advance the compare links by hand**: `[Unreleased]: .../compare/vX.Y.Z...HEAD`
  and `[X.Y.Z]: .../compare/vPREV...vX.Y.Z` (the first release links to its
  tag). Every `## [x.y.z]` heading needs its footer definition.
- [ ] `docs/releases/vX.Y.Z.md`: the notes for this release, starting with
  `# vX.Y.Z`.
- [ ] `RELEASE_NOTES.md`: add the release at the top and keep only the latest
  three.
- [ ] Read `README.md` through and correct anything the release changes
  (status, language surfaces, supported platforms and toolchains).
- [ ] Merge the release pack with an exact squash message whose body ends with
  the standard attribution footer, then check it: `git log -1 --format=%B`.
- [ ] `make release-github-preflight` (also run by `make release-preflight`):
  verifies the GitHub-hosted repository settings that live outside git:
  read-only default `GITHUB_TOKEN` permissions with PR review approval
  disabled, the active `Tag Publish Protection` ruleset on `refs/tags/v*`
  restricting tag creation, update, deletion and force-push to Organization
  Admin bypass, and GitHub immutable releases enabled.
- [ ] `make release-preflight` on the merged `main`: the checker, a clean tree
  on `main` equal to `origin/main`, `make release-github-preflight`,
  `make check`, the tool and ceremony controls, the required release-scope
  surface scan, the release notes and the changelog entry.
- [ ] Main CI is green on this exact commit. Preflight alone is not enough.

## 2. Tag

- [ ] `make release-prepare-tag-message`. It creates `vX.Y.Z.txt` in
  `TABLEPRIMS_TAG_MESSAGE_DIR` and never overwrites. Complete it: subject
  `Release X.Y.Z`, the release text, and the attribution footer.
- [ ] `make release-validate-pin`.
- [ ] `make release-tag`: the tag, signed by the committed subkey, local only,
  at `HEAD`, which must equal `origin/main`.
- [ ] `make release-verify-tag`: message bytes, tagger, target, and the
  signature against the pin committed in the tagged commit.
- [ ] `make release-push-tag`: one push, then the remote object is
  re-verified.
- [ ] `make release-verify-remote-tag`: the tag verifies against the committed
  pin and the approved primary, and GitHub reports it verified.
- [ ] Record the ceremony identity once, outside the repository, and keep
  `TABLEPRIMS_RELEASE_ANCHOR_FILE` set for every later step:
  `TABLEPRIMS_EXPECTED_TAG_OBJECT=<vX.Y.Z object> TABLEPRIMS_EXPECTED_COMMIT=<commit> make release-record-anchor`.

## 3. After the tag

- [ ] `go list -m github.com/fulmenhq/tableprims@vX.Y.Z` resolves.
- [ ] Someone other than the publisher independently runs
  `make release-validate-pin` and `make release-verify-remote-tag` (with
  `TABLEPRIMS_RELEASE_TAG=vX.Y.Z`, `TABLEPRIMS_GPG_SIGNING_FINGERPRINT`,
  `TABLEPRIMS_EXPECTED_TAG_OBJECT` and `TABLEPRIMS_EXPECTED_COMMIT` set to the
  recorded ceremony values) from a clean checkout to confirm the published tag
  object, commit, OpenPGP subkey signature, committed anchors and GitHub
  verification status match.
