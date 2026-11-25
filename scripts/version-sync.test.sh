#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Controls for version-sync.py, run on copies of the version sites. `check`
# accepts the repository and every accepted spelling, and refuses each stale
# site and each shape it does not cover; writing moves every site, is
# idempotent, and changes nothing when a site cannot be edited in place.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
python="${PYTHON:-python3}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
current="$(cat "$root/VERSION")"
xlsx="tableprims-xlsx = { version = \"=$current\", path = \"crates/tableprims-xlsx\" }"

# A fresh copy of the version sites in $tree.
fixture() {
	tree="$scratch/tree"
	rm -rf "$tree"
	mkdir -p "$tree/scripts"
	cp "$root/VERSION" "$root/Cargo.toml" "$root/Cargo.lock" "$tree/"
	cp "$root/scripts/version-sync.py" "$tree/scripts/"
	for manifest in "$root"/crates/*/Cargo.toml; do
		member="$(basename "$(dirname "$manifest")")"
		mkdir -p "$tree/crates/$member"
		cp "$manifest" "$tree/crates/$member/"
	done
}

# Replace OLD by NEW, exactly once, in FILE of the copy.
edit() {
	"$python" -I -c '
import pathlib, sys
path, old, new = pathlib.Path(sys.argv[1]), sys.argv[2], sys.argv[3]
text = path.read_text(encoding="utf-8")
if text.count(old) != 1:
    sys.exit(f"control setup: {old!r} occurs {text.count(old)} times in {path}")
path.write_text(text.replace(old, new), encoding="utf-8", newline="\n")' "$tree/$1" "$2" "$3"
}

sync() { "$python" -I -B "$tree/scripts/version-sync.py" "$@" >"$scratch/out" 2>&1; }

accepts() {
	sync check || { cat "$scratch/out" >&2; echo "error: version-check refused: $1" >&2; exit 1; }
}

refuses() {
	if sync check; then
		echo "error: version-check accepted: $1" >&2
		exit 1
	fi
}

snapshot() { cat "$tree/Cargo.toml" "$tree/Cargo.lock" >"$scratch/before"; }

unchanged() {
	cat "$tree/Cargo.toml" "$tree/Cargo.lock" | cmp -s - "$scratch/before" ||
		{ echo "error: version-sync wrote after refusing: $1" >&2; exit 1; }
}

# Accepted spellings.
fixture
accepts 'the repository'
fixture
edit Cargo.toml "$xlsx" "tableprims-xlsx = { version = '=$current', path = './crates/tableprims-xlsx' }"
accepts 'a single-quoted ./ path'
fixture
edit Cargo.toml "$xlsx"$'\n' ''
printf '\n[workspace.dependencies.tableprims-xlsx]\nversion = "=%s"\npath = "crates/tableprims-xlsx"\n' "$current" >>"$tree/Cargo.toml"
accepts 'a pin written as a table'
fixture
edit crates/tableprims-cli/Cargo.toml 'tableprims-xlsx.workspace = true' 'tableprims-xlsx = { workspace = true, features = [], optional = false }'
accepts 'an inherited dependency with features'
fixture
printf '\n[target.%s.dev-dependencies]\ntableprims.workspace = true\n' "'cfg(windows)'" >>"$tree/crates/tableprims-xlsx/Cargo.toml"
accepts 'an inherited target dev-dependency'
fixture
edit Cargo.toml 'members = [' $'members = [\n  # see [notes]\n '
accepts 'a comment holding ] in members'

# Stale sites, in every spelling.
stale() {
	fixture
	edit "$1" "$2" "$3"
	refuses "$4"
}
stale Cargo.toml "$xlsx" 'tableprims-xlsx = { version = "=0.0.9", path = "crates/tableprims-xlsx" }' 'a stale pin'
stale Cargo.toml "$xlsx" 'tableprims-xlsx  =  { version = "=0.0.9", path = "crates/tableprims-xlsx" }' 'a stale pin with extra spaces'
stale Cargo.toml "$xlsx" "tableprims-xlsx = { version = '=0.0.9', path = './crates/tableprims-xlsx' }" 'a stale single-quoted pin'
stale Cargo.toml "$xlsx" $'tableprims-xlsx.version = "=0.0.9"\ntableprims-xlsx.path = "crates/tableprims-xlsx"' 'a stale pin in dotted keys'
stale Cargo.toml "$xlsx" 'xlsx = { package = "tableprims-xlsx", version = "=0.0.9", path = "crates/tableprims-xlsx" }' 'a stale renamed pin'
stale Cargo.toml "$xlsx" "tableprims-xlsx = { version = \"$current\", path = \"crates/tableprims-xlsx\" }" 'a pin that is not exact'
stale Cargo.toml "$xlsx" 'tableprims-xlsx = { path = "crates/tableprims-xlsx" }' 'a pin with no version'
stale Cargo.toml "$xlsx" "tableprims-xlsx = { version = \"^$current\", path = \"crates/tableprims-xlsx\" }" 'a caret pin'
stale Cargo.toml "$xlsx" "tableprims-xlsx = { version = \"=$current\", path = \"crates/tableprims\" }" 'a pin with the wrong path'
stale Cargo.toml "$xlsx"$'\n' '' 'a member with no pin'
stale Cargo.toml "version = \"$current\"" 'version = "0.0.9"' 'a stale workspace version'
stale Cargo.lock $'name = "tableprims-xlsx"\nversion = "'"$current"'"' $'name = "tableprims-xlsx"\nversion = "0.0.9"' 'a stale lock entry'
stale Cargo.lock $'name = "tableprims-xlsx"\nversion = "'"$current"$'"\n' $'name = "tableprims-xlsx"\nversion = "'"$current"$'"\nsource = "registry+https://github.com/rust-lang/crates.io-index"\n' 'a lock entry from a registry'
stale crates/tableprims-xlsx/Cargo.toml $'\nversion.workspace = true' $'\nversion = "'"$current"'"' 'a member with its own version'
stale crates/tableprims-xlsx/Cargo.toml 'tableprims.workspace = true' 'tableprims = { path = "../tableprims" }' 'a member dependency by path'
stale crates/tableprims-xlsx/Cargo.toml 'tableprims.workspace = true' 'tableprims = { workspace = true, version = "0.0.9" }' 'an inherited dependency with a version'
member_table() {
	fixture
	printf '\n[%s]\ntableprims = { path = "../tableprims" }\n' "$1" >>"$tree/crates/tableprims-cli/Cargo.toml"
	refuses "a member path dependency in [$1]"
}
member_table 'build-dependencies'
member_table "target.'cfg(unix)'.dependencies"
fixture
edit crates/tableprims-xlsx/Cargo.toml '[dev-dependencies]' $'[dev-dependencies]\ntableprims = { path = "../tableprims" }'
edit crates/tableprims-xlsx/Cargo.toml $'[dependencies]\ntableprims.workspace = true\n' $'[dependencies]\n'
refuses 'a member path dependency in [dev-dependencies]'
stale Cargo.toml 'members = ["crates/tableprims", ' 'members = ["crates/*", ' 'a glob member'
fixture
echo '0.2.0-rc.1' >"$tree/VERSION"
refuses 'a pre-release VERSION'
grep -q 'VERSION must hold one stable semantic version' "$scratch/out" ||
	{ cat "$scratch/out" >&2; echo 'error: a pre-release VERSION was refused for another reason' >&2; exit 1; }

# Writing.
fixture
echo '9.8.7' >"$tree/VERSION"
sync || { cat "$scratch/out" >&2; echo 'error: version-sync refused the repository' >&2; exit 1; }
accepts 'the sites after writing 9.8.7'
grep -q '^version = "9.8.7"$' "$tree/Cargo.toml" || { echo 'error: the workspace version was not written' >&2; exit 1; }
[ "$(grep -c '"=9.8.7", path = "crates/' "$tree/Cargo.toml")" = 2 ] || { echo 'error: a member pin was not written' >&2; exit 1; }
[ ! -e "$tree/.Cargo.toml.version-sync" ] && [ ! -e "$tree/.Cargo.lock.version-sync" ] || { echo 'error: a staged file was left behind' >&2; exit 1; }
snapshot
sync
unchanged 'a second run'
for case in spaces table twice; do
	fixture
	echo '9.8.7' >"$tree/VERSION"
	if [ "$case" = spaces ]; then
		edit Cargo.toml "$xlsx" "tableprims-xlsx  = { version = \"=$current\", path = \"crates/tableprims-xlsx\" }"
	elif [ "$case" = table ]; then
		edit Cargo.toml "$xlsx"$'\n' ''
		printf '\n[workspace.dependencies.tableprims-xlsx]\nversion = "=%s"\npath = "crates/tableprims-xlsx"\n' "$current" >>"$tree/Cargo.toml"
	else
		printf '\n[patch.crates-io]\n%s\n' "$xlsx" >>"$tree/Cargo.toml"
	fi
	snapshot
	if sync; then
		echo "error: version-sync edited a pin it cannot edit in place ($case)" >&2
		exit 1
	fi
	unchanged "a pin it cannot edit in place ($case)"
done
echo 'version-sync controls: ok'
