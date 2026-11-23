# tableprims: typed tabular conversion library and the tablebolt CLI
#
# `make check` is everything CI runs on one host.

SHELL := bash
.SHELLFLAGS := -eu -o pipefail -c
CARGO ?= cargo
GO ?= go
# The Go module builds without cgo and never fetches a toolchain; the race
# detector needs cgo.
GOENV := GOWORK=off GOFLAGS=-mod=readonly GOTOOLCHAIN=local CGO_ENABLED=0
GOENV_RACE := GOWORK=off GOFLAGS=-mod=readonly GOTOOLCHAIN=local CGO_ENABLED=1
PYTHON ?= $(shell for p in python3 python3.14 python3.13 python3.12; do if command -v $$p >/dev/null 2>&1 && $$p -P -B -E -c 'import yaml' >/dev/null 2>&1; then echo $$p; exit 0; fi; done; echo python3)
export PYTHON
YAMLFMT ?= yamlfmt
YAMLLINT ?= yamllint
ACTIONLINT ?= actionlint
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
VERSION := $(shell tr -d ' \t\r\n' < VERSION)
TABLEBOLT := target/debug/tablebolt

.DEFAULT_GOAL := help
.PHONY: all help check check-platform precommit prepush pr-final fmt fmt-check lint lint-yaml lint-workflows \
        test smoke check-go test-go test-go-race msrv-check deps-check deps-check-test deny surface-scan content-check licenses-check surface-tests \
        workflow-pin release-tooling-test checklist-targets doctor doctor-test release-ceremony-test \
        release-export-pin release-validate-pin release-insert-anchors release-github-preflight \
        release-preflight release-prepare-tag-message release-tag release-push-tag release-verify-tag \
        release-verify-remote-tag release-record-anchor version version-check version-sync version-set \
        version-patch version-minor version-major build build-release clean

all: check

help: ## Show available targets
	@awk 'BEGIN{FS=":.*## "} /^[a-z][a-z0-9-]*:.*## /{printf "  %-16s %s\n", $$1, $$2}' $(MAKEFILE_LIST)

check-platform: fmt-check lint test check-go test-go smoke deps-check content-check licenses-check surface-tests checklist-targets version-check ## Platform-native subset of check (no cargo-deny or PyYAML required)

check: check-platform test-go-race workflow-pin release-tooling-test deny ## Everything CI runs on one host
	@echo "[ok] check"

precommit: fmt-check lint ## Fast pre-commit gate
	@echo "[ok] precommit"

prepush: check msrv-check lint-yaml lint-workflows deps-check-test doctor-test surface-scan ## Pre-push gate (the surface scan is skipped without a catalog)
	@echo "[ok] prepush"

# Before opening or updating a PR: everything CI runs, plus the maintainer's
# public-surface scan, which is required here, including the PR title and body
# in the file named by TABLEPRIMS_SURFACE_PR_TEXT. The scope is fixed here so
# an inherited release scope cannot waive the PR text.
pr-final: check ## Before opening or updating a PR: check plus the required surface scan
	@TABLEPRIMS_SURFACE_SCAN_REQUIRED=1 TABLEPRIMS_SURFACE_SCOPE=pr PYTHON=$(PYTHON) scripts/surface-scan.sh
	@echo "[ok] pr-final"

# The public-surface scan uses limensafe with a maintainer catalog named by
# LIMENSAFE_CATALOG_PATH, outside this repository. It never runs in CI.
surface-scan: ## Public-surface scan (tree, commit messages, PR text, branch) when LIMENSAFE_CATALOG_PATH is set
	@PYTHON=$(PYTHON) scripts/surface-scan.sh

# Vocabulary-free shapes only (local paths, keys, tokens, commit markers); runs
# in CI as part of check.
content-check: ## Refuse local paths, private keys, tokens and commit markers in tracked files
	@$(PYTHON) -I -B scripts/content-check.py

licenses-check: ## Licence of every package in the resolved graph, dev-dependencies included
	@$(PYTHON) -I -B scripts/licenses-check.py
	@PYTHON=$(PYTHON) scripts/licenses-check.test.sh

surface-tests: ## Controls for content-check and the surface scan (the scan suite skips without limensafe)
	@PYTHON=$(PYTHON) scripts/content-check.test.sh
	@PYTHON=$(PYTHON) scripts/surface-scan.test.sh

workflow-pin: ## Every workflow and local action matches its reviewed digest and is read-only
	@$(PYTHON) -P -B -E scripts/release-workflow-pin.py check

checklist-targets: ## Every make target named in RELEASE_CHECKLIST.md exists
	@scripts/checklist-targets.sh
	@scripts/checklist-targets.test.sh

release-tooling-test: ## Controls for the release tooling (synthetic; no keys or credentials)
	@PYTHON=$(PYTHON) scripts/release-workflow-pin.test.sh

# The doctor reports maintainer host tools; its controls stub them on PATH with
# symlinks, so they run on maintainer hosts (prepush), not on Windows CI.
doctor-test: ## Controls for make doctor (maintainer hosts)
	@scripts/doctor.test.sh

# The dependency-guard controls stub cargo on PATH and build a scratch Go
# module, so they need a Go toolchain and run on maintainer hosts (prepush).
deps-check-test: ## Controls for deps-check (maintainer hosts; needs go)
	@scripts/deps-check.test.sh

# Release-ceremony controls need gpg and decernor (TABLEPRIMS_DECERNOR_BIN,
# or decernor on PATH for tests). They use throwaway keys only and run on
# maintainer hosts, not in CI.
release-ceremony-test: ## Synthetic release-ceremony controls (throwaway keys; maintainer hosts)
	@scripts/release-github-preflight.test.sh
	@scripts/release-validate-pin.test.sh
	@scripts/release-anchors.test.sh
	@scripts/release-gh.test.sh
	@scripts/release-tag-controls.test.sh
	@scripts/release-verify-tag.test.sh
	@scripts/verify-pinned-tag.test.sh
	@scripts/release-verify-published-tag.test.sh

# Release
# -----------------------------------------------------------------------------
#
# A release is one signed annotated tag, vX.Y.Z, at a commit of main; it
# releases the Rust crates and the root Go module together. The tag is an
# explicit input (TABLEPRIMS_RELEASE_TAG); it is never derived from VERSION,
# git describe or the branch, and a bare RELEASE_TAG from any origin is refused
# so it cannot stand in for it.
#
# Maintainer inputs (local environment; never in this tree):
#   TABLEPRIMS_RELEASE_TAG             vX.Y.Z
#   TABLEPRIMS_TAG_MESSAGE_DIR         per-release message directory ending in the tag
#   TABLEPRIMS_DECERNOR_BIN            absolute path to decernor >= 0.1.8 (no fallback)
#   TABLEPRIMS_TAGGER_NAME/EMAIL       the committed tagger identity
#   TABLEPRIMS_PGP_KEY_ID              the committed signing subkey followed by !
#   TABLEPRIMS_GPG_HOMEDIR             external GPG home
#   TABLEPRIMS_GPG_SIGNING_FINGERPRINT the approved primary fingerprint
#   TABLEPRIMS_RELEASE_ANCHOR_FILE     external ceremony anchor record
#
# The public pins are committed (docs/security, keys/, config/release) and
# every verify step reads them from the tagged commit. CI holds no signing,
# release or registry authority.

# Release stages must not overlap under `make -j`.
.NOTPARALLEL:

ifneq ($(origin RELEASE_TAG),undefined)
$(error RELEASE_TAG is not accepted; set TABLEPRIMS_RELEASE_TAG=vX.Y.Z)
endif
REQUIRE_RELEASE_TAG = @if [ -z "$(TABLEPRIMS_RELEASE_TAG)" ]; then \
	echo "error: TABLEPRIMS_RELEASE_TAG=vX.Y.Z is required"; exit 1; fi

# Per-release notes file for the version in the tree (preflight only; not a tag).
NOTES_NAME := v$(strip $(VERSION))

release-github-preflight: ## Read-only preflight for GitHub-hosted release settings (workflow token, v* tag ruleset, immutable releases)
	@scripts/release-github-preflight.sh

release-preflight: ## Pre-tag gate on a clean main synced with origin
	@source scripts/release-decernor.sh && resolve_release_decernor && echo "[ok] checker $$RELEASE_DECERNOR_BIN"
	@test -z "$$(git status --porcelain)" || { echo "error: working tree not clean"; git status --short; exit 1; }
	@test "$$(git symbolic-ref --quiet --short HEAD)" = main || { echo "error: release from main"; exit 1; }
	git fetch --quiet origin main
	@test "$$(git rev-parse HEAD)" = "$$(git rev-parse origin/main)" || { echo "error: HEAD differs from origin/main"; exit 1; }
	$(MAKE) release-github-preflight
	$(MAKE) check doctor-test deps-check-test release-ceremony-test
	TABLEPRIMS_SURFACE_SCAN_REQUIRED=1 TABLEPRIMS_SURFACE_SCOPE=release PYTHON=$(PYTHON) scripts/surface-scan.sh
	@test -f "docs/releases/$(NOTES_NAME).md" || { echo "error: docs/releases/$(NOTES_NAME).md is required"; exit 1; }
	@grep -q '^## \[$(VERSION)\]' CHANGELOG.md || { echo "error: CHANGELOG.md has no [$(VERSION)] section"; exit 1; }
	@grep -q '^\[$(VERSION)\]: ' CHANGELOG.md || { echo "error: CHANGELOG.md has no [$(VERSION)] compare link"; exit 1; }
	@echo "[ok] release preflight; next: make release-prepare-tag-message"

release-prepare-tag-message: ## Prepare the release tag message (outside the repo)
	$(REQUIRE_RELEASE_TAG)
	scripts/release-tag-operator.sh prepare-message

release-tag: ## Create the release tag, signed and local only
	$(REQUIRE_RELEASE_TAG)
	scripts/release-tag-operator.sh local-tag

release-push-tag: ## Push the verified release tag, then verify it
	$(REQUIRE_RELEASE_TAG)
	scripts/release-tag-operator.sh remote-push

release-verify-tag: ## Verify the local release tag against the committed pin
	$(REQUIRE_RELEASE_TAG)
	scripts/release-verify-tag.sh

release-verify-remote-tag: ## Verify the pushed release tag against the committed pin and GitHub
	$(REQUIRE_RELEASE_TAG)
	scripts/release-verify-published-tag.sh

release-record-anchor: ## Record the approved tag object and commit (no overwrite)
	$(REQUIRE_RELEASE_TAG)
	scripts/release-record-anchor.sh

release-export-pin: ## One time: write the public signing pins for review (refuses overwrite)
	scripts/release-export-pin.sh

release-validate-pin: ## Validate the committed public pins and anchors
	scripts/release-validate-pin.sh

release-insert-anchors: ## Derive the fingerprint anchors with decernor (refuses overwrite)
	scripts/release-insert-anchors.sh

doctor: ## Report host tool versions against the recommended minimums (DOCTOR_SCOPE=release|dev)
	@scripts/doctor.sh $(or $(DOCTOR_SCOPE),release)

fmt: ## Format Rust and Go sources
	$(CARGO) fmt --all
	gofmt -w table

fmt-check: ## Check formatting
	$(CARGO) fmt --all -- --check

lint: ## Clippy with warnings denied
	$(CARGO) clippy --locked --workspace --all-targets -- -D warnings

lint-yaml: ## Check YAML formatting and lint
	$(YAMLFMT) -dstar -lint .yamlfmt .yamllint '.fulmen/**/*.yaml' '.github/**/*.yml' '.github/**/*.yaml' '.goneat/**/*.yaml'
	$(YAMLLINT) --strict -c .yamllint .yamlfmt .yamllint .fulmen .github .goneat

lint-workflows: ## actionlint on the workflows
	$(ACTIONLINT)

test: ## Rust tests
	$(CARGO) test --locked --workspace

check-go: ## Go: gofmt, vet and a cgo-free build of the root module
	@out="$$(gofmt -l table)"; if [ -n "$$out" ]; then echo "gofmt: $$out" >&2; exit 1; fi
	$(GOENV) $(GO) vet ./...
	$(GOENV) $(GO) build ./...

test-go: ## Go tests, the conformance vectors included, without cgo
	$(GOENV) $(GO) test -count=1 ./...

test-go-race: ## Go tests under the race detector (needs cgo and a C compiler)
	$(GOENV_RACE) $(GO) test -race -count=1 ./...

smoke: build ## Run the built tablebolt binary and check its exit codes
	$(TABLEBOLT) version --json
	$(TABLEBOLT) capabilities --json
	@status=0; $(TABLEBOLT) xlsx write >/dev/null 2>&1 || status=$$?; \
		if [ "$$status" -ne 64 ]; then \
			echo "expected exit 64 for a command without its arguments, got $$status"; exit 1; \
		fi
	@echo "[ok] tablebolt smoke passed"

msrv-check: ## Rust tests at the MSRV
	$(CARGO) +$(MSRV) test --locked --workspace

deps-check: ## Dependency rule: a dependency-free library crate, no other prim or rsfulmen beyond the CLI allowlist, no Go module dependencies
	scripts/deps-check.sh
	@echo "[ok] deps-check"

deny: ## cargo-deny: licenses, bans, sources, advisories
	$(CARGO) deny --locked check

version: ## Print the version
	@echo $(VERSION)

# VERSION is the single source of truth. Change it only through these targets;
# they write every version site (Cargo.toml, Cargo.lock) from it.
version-check: ## Every version site matches VERSION
	@$(PYTHON) -I -B scripts/version-sync.py check

version-sync: ## Write VERSION to every version site
	@$(PYTHON) -I -B scripts/version-sync.py

version-set: ## Set an explicit version (V=X.Y.Z), then sync
	@[[ "$(V)" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$$ ]] || { echo "usage: make version-set V=X.Y.Z" >&2; exit 1; }
	@printf '%s\n' "$(V)" >VERSION
	@$(PYTHON) -I -B scripts/version-sync.py

version-patch: ## Bump the patch version, then sync
	@IFS=. read -r major minor patch <VERSION; printf '%s\n' "$$major.$$minor.$$((patch + 1))" >VERSION
	@$(PYTHON) -I -B scripts/version-sync.py

version-minor: ## Bump the minor version, then sync
	@IFS=. read -r major minor patch <VERSION; printf '%s\n' "$$major.$$((minor + 1)).0" >VERSION
	@$(PYTHON) -I -B scripts/version-sync.py

version-major: ## Bump the major version, then sync
	@IFS=. read -r major minor patch <VERSION; printf '%s\n' "$$((major + 1)).0.0" >VERSION
	@$(PYTHON) -I -B scripts/version-sync.py

build: ## Build all crates (debug)
	$(CARGO) build --locked --workspace

build-release: ## Build all crates (release)
	$(CARGO) build --locked --workspace --release

clean: ## Remove build output
	$(CARGO) clean
