#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Every `make <target>` that RELEASE_CHECKLIST.md names must be a target in
# the Makefile, so the checklist cannot drift from the commands it describes.
# Needs no keys or tools beyond git and grep, so it runs in make check.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
checklist="${1:-$root/RELEASE_CHECKLIST.md}"
makefile="${2:-$root/Makefile}"
for file in "$checklist" "$makefile"; do
	[[ -f "$file" && -r "$file" ]] || {
		echo "error: cannot read $file" >&2
		exit 1
	}
done
# Extract synchronously so a failed extraction fails the check. grep exits 1
# when nothing matches, which is also an error: the checklist names targets.
names="$(grep -oE 'make [a-z][a-z0-9-]+' "$checklist")" || {
	echo "error: no make targets found in $checklist" >&2
	exit 1
}
targets="$(printf '%s\n' "$names" | awk '{print $2}' | sort -u)"
[[ -n "$targets" ]] || {
	echo "error: no make targets found in $checklist" >&2
	exit 1
}
missing=0
while IFS= read -r target; do
	grep -Eq "^${target}:" "$makefile" || {
		echo "error: RELEASE_CHECKLIST.md names missing make target $target" >&2
		missing=1
	}
done <<<"$targets"
[[ "$missing" -eq 0 ]] || exit 1
echo '[ok] every make target named in RELEASE_CHECKLIST.md exists'
