#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Controls for checklist-targets.sh and for the scope pr-final gives the
# surface scan. Each negative control must fail; a pass here would mean the
# check can succeed without doing its work.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
check="$root/scripts/checklist-targets.sh"
tmp="$(mktemp -d)"
trap 'rm -rf -- "$tmp"' EXIT

refused() {
	local label="$1"
	shift
	if "$@" >/dev/null 2>&1; then
		echo "error: control passed but must fail: $label" >&2
		exit 1
	fi
}

printf 'demo: ## a target\n\t@true\n' >"$tmp/Makefile"
printf 'Run make demo first.\n' >"$tmp/good.md"
printf 'Run make absent-target first.\n' >"$tmp/missing-target.md"
printf 'This checklist names no targets.\n' >"$tmp/no-targets.md"

"$check" "$tmp/good.md" "$tmp/Makefile" >/dev/null
refused "checklist names a target the Makefile lacks" "$check" "$tmp/missing-target.md" "$tmp/Makefile"
refused "checklist names no targets" "$check" "$tmp/no-targets.md" "$tmp/Makefile"
refused "checklist file is absent" "$check" "$tmp/absent.md" "$tmp/Makefile"
refused "Makefile is absent" "$check" "$tmp/good.md" "$tmp/absent-Makefile"

# pr-final must scan in PR scope whatever the caller's environment says, so an
# inherited release scope cannot waive the PR title and body.
recipe="$(grep -A3 '^pr-final:' "$root/Makefile")"
grep -q 'TABLEPRIMS_SURFACE_SCAN_REQUIRED=1 TABLEPRIMS_SURFACE_SCOPE=pr ' <<<"$recipe" || {
	echo "error: pr-final does not fix the surface scan to PR scope" >&2
	exit 1
}

echo '[ok] checklist-targets: missing target, no targets, absent files; pr-final scans in PR scope'
