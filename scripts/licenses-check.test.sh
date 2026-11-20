#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# The licence check passes the real graph and a synthetic graph whose
# licences are allowed, and fails each synthetic graph with one package
# whose licence is not allowed, including a dev-only package.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd -P)"
python="${PYTHON:-python3}"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT
check() { "$python" -I -B "$root/scripts/licenses-check.py" "$@" >"$scratch/out" 2>&1; }

# A graph of one workspace member and one dependency with `licence`.
graph() {
	"$python" -I -c '
import json, sys
licence = None if sys.argv[1] == "null" else sys.argv[1]
member = {"id": "own 0.1.0", "name": "own", "version": "0.1.0", "license": "GPL-3.0"}
dep = {"id": "dep 1.0.0", "name": "dep", "version": "1.0.0", "license": licence}
print(json.dumps({
    "workspace_members": ["own 0.1.0"],
    "packages": [member, dep],
    "resolve": {"nodes": [{"id": "own 0.1.0"}, {"id": "dep 1.0.0"}]},
}))' "$1" >"$scratch/graph.json"
}

check || { cat "$scratch/out" >&2; echo 'error: the real graph was refused' >&2; exit 1; }
for good in 'MIT' 'MIT OR Apache-2.0' 'MIT/Apache-2.0' 'Apache-2.0 WITH LLVM-exception' \
	'(MIT OR GPL-3.0) AND BSD-3-Clause' 'MIT OR GPL-3.0' 'MIT-0'; do
	graph "$good"
	check --metadata "$scratch/graph.json" || { cat "$scratch/out" >&2; echo "error: refused: $good" >&2; exit 1; }
done
for bad in 'GPL-3.0' 'MIT AND GPL-3.0' 'GPL-2.0 WITH Classpath-exception-2.0' 'MIT WITH Foo' \
	'null' '' 'MIT OR' '(MIT' 'MIT)' 'AND MIT' 'BSD-2-Clause'; do
	graph "$bad"
	if check --metadata "$scratch/graph.json"; then
		echo "error: accepted: ${bad:-<empty>}" >&2
		exit 1
	fi
done
echo 'licenses-check controls: ok'
