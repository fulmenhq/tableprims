# tableprims Makefile

.DEFAULT_GOAL := help
SHELL := bash

CARGO ?= cargo
MSRV := $(shell sed -n 's/^rust-version = "\(.*\)"/\1/p' Cargo.toml)
VERSION := $(shell tr -d ' \t\r\n' < VERSION)
TABLEBOLT := target/debug/tablebolt

.PHONY: all help build build-release clean fmt fmt-check lint test smoke \
	version version-check check deny msrv precommit prepush

all: check ## Run all quality checks

help: ## Show this help
	@grep -E '^[a-zA-Z_-]+:.*## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*## "} {printf "  %-16s %s\n", $$1, $$2}'

build: ## Build all crates (debug)
	$(CARGO) build --locked --workspace

build-release: ## Build all crates (release)
	$(CARGO) build --locked --workspace --release

clean: ## Remove build artifacts
	$(CARGO) clean

fmt: ## Format Rust sources
	$(CARGO) fmt --all

fmt-check: ## Check Rust formatting without modifying files
	$(CARGO) fmt --all -- --check

lint: ## Run Clippy with warnings denied
	$(CARGO) clippy --locked --workspace --all-targets -- -D warnings

test: ## Run the test suite
	$(CARGO) test --locked --workspace

smoke: build ## Run the built tablebolt binary and check its exit codes
	$(TABLEBOLT) version --json
	$(TABLEBOLT) capabilities --json
	@status=0; $(TABLEBOLT) xlsx write >/dev/null 2>&1 || status=$$?; \
		if [ "$$status" -ne 64 ]; then \
			echo "expected exit 64 for an unimplemented command, got $$status"; exit 1; \
		fi
	@echo "[ok] tablebolt smoke passed"

version: ## Print the current version
	@echo "$(VERSION)"

version-check: ## Check that VERSION matches Cargo.toml
	@cargo_version="$$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"; \
		if [ -z "$(VERSION)" ] || [ "$(VERSION)" != "$$cargo_version" ]; then \
			echo "VERSION ($(VERSION)) and Cargo.toml ($$cargo_version) differ"; exit 1; \
		fi
	@echo "[ok] version $(VERSION)"

check: fmt-check lint test smoke version-check ## Run all quality checks
	@echo "[ok] All quality checks passed"

deny: ## Run cargo-deny license, advisory, ban and source checks
	$(CARGO) deny --locked check

msrv: ## Build and test on the minimum supported Rust version
	$(CARGO) +$(MSRV) test --locked --workspace

precommit: fmt-check lint ## Run pre-commit checks (fast)

prepush: check deny msrv ## Run pre-push checks (thorough)
	@echo "[ok] Pre-push checks passed"
