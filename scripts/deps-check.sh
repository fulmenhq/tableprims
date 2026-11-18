#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# The library crate has no dependencies at all. No crate depends on rsfulmen
# or on another prim, with one exception: the CLI package may use the two
# canonicalization crates below at the exact version given, and only the CLI.
# When a root Go module is present, it uses the standard library only.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "${root}"
fail=0

# Shared by the manifest check and the resolved-graph check.
rules='
import re
FORBIDDEN = re.compile(r"^(?:[a-z0-9_-]*prims|rsfulmen)(?:[-_][a-z0-9_-]*)?$")
ALLOWED = {"canonprims-core": "0.1.5", "canonprims-json": "0.1.5"}
CLI_MANIFEST = "crates/tableprims-cli/Cargo.toml"
LEAF = "tableprims"
NO_PRIMS = ("tableprims", "tableprims-xlsx")
def own(name):
    return name == "tableprims" or name.startswith("tableprims-")
'

# No dependency of any kind (normal, dev, build, target-specific) on another
# *prims crate or on rsfulmen, by package name, in any workspace manifest,
# other than the allowed pair where it is allowed.
shopt -s nullglob
manifests=(Cargo.toml crates/*/Cargo.toml bindings/*/Cargo.toml)
shopt -u nullglob
if ! python3 -I -c "${rules}"'
import sys, tomllib
KEYS = ("dependencies", "dev-dependencies", "dev_dependencies", "build-dependencies", "build_dependencies")
bad = []
def tables(doc):
    for key in KEYS:
        yield key, doc.get(key, {})
    for target, spec in doc.get("target", {}).items():
        for key in KEYS:
            yield f"target.{target}.{key}", spec.get(key, {})
def allowed(path, table, name, spec):
    if name not in ALLOWED:
        return False
    exact = "=" + ALLOWED[name]
    if path == "Cargo.toml" and table == "workspace.dependencies":
        return spec == exact or spec == {"version": exact}
    if path == CLI_MANIFEST and table == "dependencies":
        return spec == {"workspace": True} or spec == exact or spec == {"version": exact}
    return False
for path in sys.argv[1:]:
    doc = tomllib.load(open(path, "rb"))
    # The XLSX adapter turns a panic of its workbook writer into an error,
    # which needs a build that unwinds.
    for profile, settings in doc.get("profile", {}).items():
        if isinstance(settings, dict) and settings.get("panic", "unwind") != "unwind":
            bad.append(f"{path}: [profile.{profile}] panic must be unwind")
    sources = list(tables(doc)) + [(f"workspace.{k}", v) for k, v in tables(doc.get("workspace", {}))]
    for table, deps in sources:
        for alias, spec in deps.items():
            name = spec.get("package", alias) if isinstance(spec, dict) else alias
            lowered = name.lower()
            if FORBIDDEN.match(lowered) and not own(lowered) and not allowed(path, table, name, spec):
                bad.append(f"{path}: [{table}] {alias}")
for line in bad:
    print(line)
sys.exit(1 if bad else 0)
' ${manifests[@]+"${manifests[@]}"}
then
  echo "deps-check: a tableprims crate depends on another prim or on rsfulmen, or a profile does not unwind on panic" >&2
  fail=1
fi
# The resolved graph (every edge kind). Failing to read it fails the check.
shopt -s nullglob
meta_manifests=(Cargo.toml bindings/*/Cargo.toml)
shopt -u nullglob
for manifest in ${meta_manifests[@]+"${meta_manifests[@]}"}; do
  if ! metadata="$(cargo metadata --manifest-path "${manifest}" --locked --format-version 1 2>/dev/null)"; then
    echo "deps-check: cargo metadata failed; the dependency graph was not checked" >&2
    fail=1
  elif ! printf '%s' "$metadata" | python3 -I -c "${rules}"'
import json, sys
doc = json.load(sys.stdin)
packages = doc["packages"]
bad = []
for package in packages:
    name = package["name"].lower()
    if FORBIDDEN.match(name) and not own(name):
        if package["name"] not in ALLOWED or ALLOWED[package["name"]] != package.get("version"):
            bad.append((name + " " + (package.get("version") or "")).strip())
by_id = {package.get("id"): package["name"] for package in packages}
nodes = {node["id"]: node.get("dependencies", []) for node in (doc.get("resolve") or {}).get("nodes", [])}
def closure(start):
    seen, todo = set(), list(nodes.get(start, []))
    while todo:
        current = todo.pop()
        if current not in seen:
            seen.add(current)
            todo.extend(nodes.get(current, []))
    return {by_id.get(item, item) for item in seen}
for start, name in by_id.items():
    if start not in nodes:
        continue
    reached = closure(start)
    if name == LEAF and reached:
        bad.append(f"{LEAF} has dependencies: " + ", ".join(sorted(reached)[:5]))
    if name in NO_PRIMS:
        prims = sorted(item for item in reached if FORBIDDEN.match(item.lower()) and not own(item.lower()))
        if prims:
            bad.append(f"{name} reaches " + ", ".join(prims))
print("\n".join(sorted(set(bad))))
sys.exit(1 if bad else 0)'; then
    echo "deps-check: a prim or rsfulmen appears in the resolved dependency graph where it is not allowed, or the graph could not be read" >&2
    fail=1
  fi
done

# The allowed pair is named in one source file only.
named="$(grep -rlE 'canonprims' crates --include='*.rs' 2>/dev/null | sort | tr '\n' ' ' || true)"
if [[ -n "${named// /}" && "${named}" != "crates/tableprims-cli/src/digest.rs " ]]; then
  echo "deps-check: canonprims is named outside crates/tableprims-cli/src/digest.rs: ${named}" >&2
  fail=1
fi

if [[ -f go.mod ]]; then
  if grep -nE '^[[:space:]]*(require|replace)\b' go.mod; then
    echo "deps-check: go.mod must have no require or replace directives" >&2
    fail=1
  fi
  for forbidden_path in go.sum go.work vendor; do
    if [[ -e "${forbidden_path}" ]]; then
      echo "deps-check: ${forbidden_path} must not exist in a zero-dependency Go module" >&2
      fail=1
    fi
  done
  own='github.com/fulmenhq/tableprims(/[A-Za-z0-9_./-]*)?'
  own_line="^${own}(\.test)?( \[${own}\.test\])?$"
  for platform in linux/amd64 linux/arm64 darwin/arm64 darwin/amd64 windows/amd64 windows/arm64; do
    for cgo in 0 1; do
      cell="GOOS=${platform%/*} GOARCH=${platform#*/} CGO_ENABLED=${cgo}"
      if ! go_deps="$(env GOWORK=off GOFLAGS=-mod=readonly GOOS="${platform%/*}" GOARCH="${platform#*/}" CGO_ENABLED="${cgo}" \
        go list -deps -test -f '{{if not .Standard}}{{.ImportPath}}{{end}}' ./... 2>&1)"; then
        echo "deps-check: go list failed under ${cell}; the Go dependency graph could not be read" >&2
        printf '%s\n' "$go_deps" | head -5 >&2
        fail=1
      elif [ -n "$(printf '%s\n' "$go_deps" | grep -Ev "${own_line}" | grep -v '^$' || true)" ]; then
        echo "deps-check: the Go module imports a non-standard package under ${cell}" >&2
        printf '%s\n' "$go_deps" | grep -Ev "${own_line}" | grep -v '^$' | head -5 >&2
        fail=1
      fi
      if cgo_files="$(env GOWORK=off GOFLAGS=-mod=readonly GOOS="${platform%/*}" GOARCH="${platform#*/}" CGO_ENABLED="${cgo}" \
        go list -test -f '{{if .CgoFiles}}{{.ImportPath}}: {{join .CgoFiles " "}}{{end}}' ./... 2>/dev/null)" &&
        [ -n "$(printf '%s' "$cgo_files" | tr -d '[:space:]')" ]; then
        echo "deps-check: the Go module uses cgo (import \"C\") under ${cell}" >&2
        printf '%s\n' "$cgo_files" | grep -v '^$' | head -5 >&2
        fail=1
      fi
    done
  done
fi
exit "${fail}"
