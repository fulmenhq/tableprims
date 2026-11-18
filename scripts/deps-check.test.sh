#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# The dependency guard refuses another *prims crate or rsfulmen however it is
# declared, accepts the workspace's own crates, and accepts the allowed pair
# only in the CLI package at its exact version.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
git -C "$root" archive HEAD Cargo.toml Cargo.lock crates | tar -x -C "$scratch"
cp "$root/Cargo.toml" "$scratch/Cargo.toml"
cp "$root/crates/tableprims/Cargo.toml" "$scratch/crates/tableprims/Cargo.toml"
cp "$root/crates/tableprims-cli/Cargo.toml" "$scratch/crates/tableprims-cli/Cargo.toml"
cp "$root/crates/tableprims-xlsx/Cargo.toml" "$scratch/crates/tableprims-xlsx/Cargo.toml"
cp "$root/Cargo.lock" "$scratch/Cargo.lock"
mkdir -p "$scratch/scripts"
cp "$root/scripts/deps-check.sh" "$scratch/scripts/"
manifest="$scratch/crates/tableprims-cli/Cargo.toml"
cp "$manifest" "$scratch/cli.toml"
cp "$scratch/Cargo.toml" "$scratch/root.toml"
cp "$scratch/crates/tableprims/Cargo.toml" "$scratch/core.toml"
cp "$scratch/crates/tableprims-xlsx/Cargo.toml" "$scratch/xlsx.toml"
restore() {
	cp "$scratch/cli.toml" "$manifest"
	cp "$scratch/root.toml" "$scratch/Cargo.toml"
	cp "$scratch/core.toml" "$scratch/crates/tableprims/Cargo.toml"
	cp "$scratch/xlsx.toml" "$scratch/crates/tableprims-xlsx/Cargo.toml"
}
check() { (cd "$scratch" && scripts/deps-check.sh) >"$scratch/out" 2>&1; }
check || { cat "$scratch/out" >&2; echo 'error: pristine tree refused' >&2; exit 1; }
refused() {
	cp "$scratch/cli.toml" "$manifest"
	printf '%s\n' "$1" >>"$manifest"
	if check; then
		echo "error: accepted: $1" >&2
		exit 1
	fi
	grep -q 'depends on another prim or on rsfulmen' "$scratch/out" || { echo "error: refused for another reason: $1" >&2; cat "$scratch/out" >&2; exit 1; }
}
refused $'[dev-dependencies.rsfulmen]\nversion = "1" # : tableprims-'
refused $'[dev-dependencies.other-prims]\nversion = "1"'
refused $'[dev-dependencies.other_prims]\nversion = "1"'
refused $'[dev-dependencies.x]\npackage = "sysprims"\nversion = "1"'
refused $'[build-dependencies.examplprims-core]\nversion = "1"'
refused $'[target.\'cfg(unix)\'.dependencies.someprims]\nversion = "1"'
refused $'[dev_dependencies.other-prims]\nversion = "1"'
refused $'[build_dependencies.rsfulmen]\nversion = "1"'
refused $'[target.\'cfg(unix)\'.dev_dependencies.someprims]\nversion = "1"'
# The resolved graph cannot be read (a lockfile that no longer matches): the
# check fails rather than skipping the graph.
cp "$scratch/cli.toml" "$manifest"
printf '%s\n' '[dev-dependencies.nonexistent_crate_xyz]' 'version = "1"' >>"$manifest"
if check; then
	echo 'error: accepted with an unreadable dependency graph' >&2
	exit 1
fi
grep -q 'cargo metadata failed' "$scratch/out" || { echo 'error: graph failure not reported' >&2; cat "$scratch/out" >&2; exit 1; }
cp "$scratch/cli.toml" "$manifest"

# The allowed pair: only in the CLI package's normal dependencies, only at the
# exact version, and named in one source file.
refused_in() {
	restore
	printf '%s\n' "$2" >>"$scratch/$1"
	if check; then
		echo "error: accepted in $1: $2" >&2
		exit 1
	fi
	grep -q 'depends on another prim or on rsfulmen' "$scratch/out" || { echo "error: refused for another reason in $1: $2" >&2; cat "$scratch/out" >&2; exit 1; }
}
refused_in crates/tableprims/Cargo.toml $'[dependencies.canonprims-core]\nworkspace = true'
refused_in crates/tableprims/Cargo.toml $'[dev-dependencies.canonprims-json]\nworkspace = true'
refused_in crates/tableprims-xlsx/Cargo.toml $'[dependencies.canonprims-json]\nworkspace = true'
refused_in crates/tableprims-xlsx/Cargo.toml $'[dev-dependencies.canonprims-core]\nversion = "=0.1.5"'
refused_in crates/tableprims-cli/Cargo.toml $'[dev-dependencies.canonprims-cbor]\nversion = "=0.1.5"'
refused_in crates/tableprims-cli/Cargo.toml $'[build-dependencies.canonprims-core]\nworkspace = true'
refused_in crates/tableprims-cli/Cargo.toml $'[dependencies.canon]\npackage = "canonprims-yaml"\nversion = "=0.1.5"'
for version in '=0.1.4' '0.1.5' '^0.1.5' '>=0.1.5'; do
	restore
	sed -i.bak "s/^canonprims-core = \"=0.1.5\"/canonprims-core = \"${version}\"/" "$scratch/Cargo.toml"
	grep -q "^canonprims-core = \"${version}\"" "$scratch/Cargo.toml" || { echo 'error: the version control did not apply' >&2; exit 1; }
	if check; then
		echo "error: accepted canonprims-core at ${version}" >&2
		exit 1
	fi
	grep -q 'depends on another prim or on rsfulmen' "$scratch/out" || { echo "error: ${version} refused for another reason" >&2; cat "$scratch/out" >&2; exit 1; }
done
restore
printf '[profile.release]\npanic = "abort"\n' >>"$scratch/Cargo.toml"
if check; then
	echo 'error: accepted a profile that aborts on panic' >&2
	exit 1
fi
grep -q 'panic must be unwind' "$scratch/out" || { echo 'error: panic profile refusal not reported' >&2; cat "$scratch/out" >&2; exit 1; }
restore
check || { cat "$scratch/out" >&2; echo 'error: restored tree refused' >&2; exit 1; }
printf '// uses canonprims_json\n' >"$scratch/crates/tableprims/src/extra.rs"
if check; then
	echo 'error: accepted canonprims named in another source file' >&2
	exit 1
fi
grep -q 'canonprims is named outside' "$scratch/out" || { echo 'error: source refusal not reported' >&2; cat "$scratch/out" >&2; exit 1; }
rm "$scratch/crates/tableprims/src/extra.rs"

# A readable graph that names a forbidden package (a transitive dependency the
# manifests do not show) is refused.
mkdir -p "$scratch/bin"
real_cargo="$(command -v cargo)"
# shellcheck disable=SC2016 # expands in the stub
printf '#!/bin/bash\nif [[ "$1" == metadata ]]; then echo %q; exit 0; fi\nexec %q "$@"\n' \
	'{"packages":[{"name":"tableprims"},{"name":"other-prims"}]}' "$real_cargo" >"$scratch/bin/cargo"
chmod +x "$scratch/bin/cargo"
if (cd "$scratch" && PATH="$scratch/bin:$PATH" scripts/deps-check.sh) >"$scratch/out" 2>&1; then
	echo 'error: accepted a forbidden package in the resolved graph' >&2
	exit 1
fi
grep -q 'appears in the resolved dependency graph' "$scratch/out" || { echo 'error: graph refusal not reported' >&2; cat "$scratch/out" >&2; exit 1; }
# The allowed pair at another version, a dependency of the library crate and
# the pair reached from the adapter are refused in the resolved graph.
graph_refused() {
	# shellcheck disable=SC2016 # expands in the stub
	printf '#!/bin/bash\nif [[ "$1" == metadata ]]; then echo %q; exit 0; fi\nexec %q "$@"\n' "$2" "$real_cargo" >"$scratch/bin/cargo"
	if (cd "$scratch" && PATH="$scratch/bin:$PATH" scripts/deps-check.sh) >"$scratch/out" 2>&1; then
		echo "error: accepted a graph with $1" >&2
		exit 1
	fi
	grep -q "$3" "$scratch/out" || { echo "error: graph refusal not reported for $1" >&2; cat "$scratch/out" >&2; exit 1; }
}
graph_refused 'the pair at another version' \
	'{"packages":[{"name":"tableprims","version":"0.1.0"},{"name":"canonprims-core","version":"0.1.4"}]}' 'canonprims-core 0.1.4'
graph_refused 'a dependency of the library crate' \
	'{"packages":[{"name":"tableprims","id":"a"},{"name":"serde","id":"b"}],"resolve":{"nodes":[{"id":"a","dependencies":["b"]},{"id":"b","dependencies":[]}]}}' 'tableprims has dependencies: serde'
graph_refused 'the pair reached from the adapter' \
	'{"packages":[{"name":"tableprims-xlsx","id":"a"},{"name":"zip","id":"b"},{"name":"canonprims-core","version":"0.1.5","id":"c"}],"resolve":{"nodes":[{"id":"a","dependencies":["b"]},{"id":"b","dependencies":["c"]},{"id":"c","dependencies":[]}]}}' 'tableprims-xlsx reaches canonprims-core'
# shellcheck disable=SC2016 # expands in the stub
printf '#!/bin/bash\nif [[ "$1" == metadata ]]; then echo %q; exit 0; fi\nexec %q "$@"\n' \
	'{"packages":[{"name":"tableprims","id":"a"},{"name":"tableprims-cli","id":"b"},{"name":"canonprims-core","version":"0.1.5","id":"c"},{"name":"serde","id":"d"}],"resolve":{"nodes":[{"id":"a","dependencies":[]},{"id":"b","dependencies":["a","c","d"]},{"id":"c","dependencies":[]},{"id":"d","dependencies":[]}]}}' "$real_cargo" >"$scratch/bin/cargo"
(cd "$scratch" && PATH="$scratch/bin:$PATH" scripts/deps-check.sh) >"$scratch/out" 2>&1 || { echo 'error: refused a clean graph' >&2; cat "$scratch/out" >&2; exit 1; }

# Go module checks: a clean zero-dependency module passes, while require/replace
# directives, go.sum, go.work, or vendor/ are refused.
mkdir -p "$scratch/yaml/internal/engine"
printf 'module github.com/fulmenhq/tableprims\n\ngo 1.21\n' >"$scratch/go.mod"
printf 'package engine\n' >"$scratch/yaml/internal/engine/stub.go"
check || { cat "$scratch/out" >&2; echo 'error: clean Go module refused' >&2; exit 1; }

printf 'module github.com/fulmenhq/tableprims\n\ngo 1.21\n\nrequire example.com/ext v1.0.0\n' >"$scratch/go.mod"
if check; then
	echo 'error: accepted require directive in go.mod' >&2
	exit 1
fi
grep -q 'go.mod must have no require or replace directives' "$scratch/out" || { echo 'error: go.mod require refusal not reported' >&2; cat "$scratch/out" >&2; exit 1; }
printf 'module github.com/fulmenhq/tableprims\n\ngo 1.21\n' >"$scratch/go.mod"

for forbidden_path in go.sum go.work; do
	: >"$scratch/$forbidden_path"
	if check; then
		echo "error: accepted $forbidden_path" >&2
		exit 1
	fi
	grep -q "$forbidden_path must not exist" "$scratch/out" || { echo "error: $forbidden_path refusal not reported" >&2; cat "$scratch/out" >&2; exit 1; }
	rm -f "$scratch/$forbidden_path"
done

mkdir -p "$scratch/vendor"
if check; then
	echo 'error: accepted vendor/' >&2
	exit 1
fi
grep -q 'vendor must not exist' "$scratch/out" || { echo 'error: vendor refusal not reported' >&2; cat "$scratch/out" >&2; exit 1; }
rmdir "$scratch/vendor"

echo '[ok] dependency guard: comments, dev/build/target tables and aliases, renames, separators, the allowed pair by package, table, version and source file, unreadable graph, and transitive graph checks'
