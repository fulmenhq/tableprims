#!/usr/bin/env python3
# SPDX-License-Identifier: MIT OR Apache-2.0
"""Keep every version site in step with VERSION, the single source of truth.

Usage:
  version-sync.py          write VERSION into Cargo.toml and Cargo.lock
  version-sync.py check    fail if any site differs from VERSION

The manifests are read as TOML. The workspace members are those listed in
`[workspace] members`; each must inherit the workspace version, and must
depend on another member only through `workspace = true` (with `features`,
`optional` or `default-features` beside it, if any). The sites are:

- the `[workspace.package]` version;
- the exact version (`=X.Y.Z`) of every `[workspace.dependencies]` entry that
  names a member, which must also carry the member's path;
- the Cargo.lock entry of every member.

Any other shape is refused, not skipped. Writing edits the canonical spelling
of each site in memory and checks the result before anything is written; a
site it cannot edit is an error and nothing is written. Each file is then
replaced whole, Cargo.toml first; if replacing Cargo.lock fails, `check`
reports it and a second run completes it. Nothing outside these files is
touched. scripts/version-sync.test.sh holds the controls.
"""

import os
import re
import sys
import tomllib
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parent.parent
SEMVER = re.compile(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)")
DEPENDENCY_TABLES = ("dependencies", "dev-dependencies", "build-dependencies")
# Keys Cargo accepts beside `workspace = true` in a member's dependency.
INHERITED_KEYS = {"workspace", "features", "optional", "default-features"}
WORKSPACE_PACKAGE_VERSION = re.compile(r'(?ms)^(\[workspace\.package\]\n(?:(?!^\[).)*?^version = )"[^"]*"')


def read_version():
    version = (ROOT / "VERSION").read_text(encoding="utf-8").strip()
    if not SEMVER.fullmatch(version):
        raise ValueError("VERSION must hold one stable semantic version")
    return version


def normalize(path):
    parts = [p for p in PurePosixPath(path).parts if p != "."]
    if not parts or ".." in parts or PurePosixPath(path).is_absolute():
        raise ValueError(f"workspace path {path!r} must be relative and inside the repository")
    return "/".join(parts)


def members(workspace):
    """Return {path: package name} for every workspace member."""
    listed = workspace.get("members")
    if not isinstance(listed, list) or not listed:
        raise ValueError("Cargo.toml lists no [workspace] members")
    found = {}
    for path in listed:
        if not isinstance(path, str) or any(c in path for c in "*?["):
            raise ValueError(f"workspace member {path!r} must be a plain path")
        path = normalize(path)
        manifest = tomllib.loads((ROOT / path / "Cargo.toml").read_text(encoding="utf-8"))
        package = manifest.get("package", {})
        name = package.get("name")
        if not isinstance(name, str):
            raise ValueError(f"{path}/Cargo.toml has no [package] name")
        if package.get("version") != {"workspace": True}:
            raise ValueError(f"{path}/Cargo.toml must set version.workspace = true")
        found[path] = (name, manifest)
    return found


def member_dependencies(manifest):
    """Yield (table, key, spec) for every dependency of a member manifest."""
    tables = [(t, manifest.get(t, {})) for t in DEPENDENCY_TABLES]
    for target, spec in manifest.get("target", {}).items():
        tables += [(f"target.{target}.{t}", spec.get(t, {})) for t in DEPENDENCY_TABLES]
    for table, deps in tables:
        for key, spec in deps.items():
            yield table, key, spec


def sites(text_toml, text_lock):
    """Return {site: version} for every version site; refuse any other shape."""
    workspace = tomllib.loads(text_toml).get("workspace")
    if not isinstance(workspace, dict):
        raise ValueError("Cargo.toml has no [workspace]")
    found = {}
    version = workspace.get("package", {}).get("version")
    if not isinstance(version, str):
        raise ValueError("Cargo.toml has no [workspace.package] version")
    found["Cargo.toml [workspace.package]"] = version

    by_path = members(workspace)
    names = {name for name, _ in by_path.values()}
    pinned = set()
    for key, spec in workspace.get("dependencies", {}).items():
        spec = spec if isinstance(spec, dict) else {"version": spec}
        package = spec.get("package", key)
        path = spec.get("path")
        if path is None and package not in names:
            continue
        if path is None or normalize(path) not in by_path or by_path[normalize(path)][0] != package:
            raise ValueError(f"Cargo.toml [workspace.dependencies] {key} must name a member by its path")
        exact = spec.get("version")
        if not isinstance(exact, str) or not exact.startswith("="):
            raise ValueError(f'Cargo.toml [workspace.dependencies] {key} must pin version = "=X.Y.Z"')
        found[f"Cargo.toml [workspace.dependencies] {key}"] = exact[1:]
        pinned.add(package)

    for path, (name, manifest) in by_path.items():
        for table, key, spec in member_dependencies(manifest):
            package = spec.get("package", key) if isinstance(spec, dict) else key
            local = isinstance(spec, dict) and "path" in spec
            inherited = isinstance(spec, dict) and spec.get("workspace") is True and set(spec) <= INHERITED_KEYS
            if (package in names or local) and not inherited:
                raise ValueError(f"{path}/Cargo.toml [{table}] {key} must be {key}.workspace = true")
            if package in names and package not in pinned:
                raise ValueError(f"Cargo.toml [workspace.dependencies] has no entry for {package}")

    lock = tomllib.loads(text_lock).get("package", [])
    for name in sorted(names):
        entries = [p for p in lock if p.get("name") == name and "source" not in p]
        if len(entries) != 1:
            raise ValueError(f"Cargo.lock must hold exactly one {name} entry (found {len(entries)})")
        found[f"Cargo.lock {name}"] = entries[0].get("version")
    return found, names, pinned


def replace_one(pattern, text, replacement, site):
    text, count = pattern.subn(replacement, text, count=2)
    if count != 1:
        raise ValueError(f"cannot edit {site} in place (found {count} canonical sites); set it by hand")
    return text


def write(version, text_toml, text_lock):
    _, names, pinned = sites(text_toml, text_lock)
    text_toml = replace_one(WORKSPACE_PACKAGE_VERSION, text_toml, lambda m: f'{m.group(1)}"{version}"', "[workspace.package] version")
    for key in tomllib.loads(text_toml)["workspace"].get("dependencies", {}):
        spec = tomllib.loads(text_toml)["workspace"]["dependencies"][key]
        if isinstance(spec, dict) and spec.get("package", key) in pinned and "path" in spec:
            pattern = re.compile(r'(?m)^(' + re.escape(key) + r' = \{ version = )"=[^"]*"')
            text_toml = replace_one(pattern, text_toml, lambda m: f'{m.group(1)}"={version}"', f"[workspace.dependencies] {key}")
    for name in names:
        pattern = re.compile(r'(?m)^(\[\[package\]\]\nname = "' + re.escape(name) + r'"\nversion = )"[^"]*"(?=\n(?!source))')
        text_lock = replace_one(pattern, text_lock, lambda m: f'{m.group(1)}"{version}"', f"Cargo.lock {name}")
    wrong = {site: v for site, v in sites(text_toml, text_lock)[0].items() if v != version}
    if wrong:
        raise ValueError(f"after editing, still not {version}: {', '.join(sorted(wrong))}; set them by hand")
    return text_toml, text_lock


def main():
    args = sys.argv[1:]
    if args not in ([], ["check"]):
        raise ValueError("usage: version-sync.py [check]")
    version = read_version()
    toml_path, lock_path = ROOT / "Cargo.toml", ROOT / "Cargo.lock"
    text_toml = toml_path.read_text(encoding="utf-8")
    text_lock = lock_path.read_text(encoding="utf-8")
    if args == ["check"]:
        found, _, _ = sites(text_toml, text_lock)
        wrong = {site: v for site, v in found.items() if v != version}
        for site, v in wrong.items():
            print(f"error: {site} is {v}; VERSION is {version}", file=sys.stderr)
        if wrong:
            sys.exit("version-check: run make version-sync")
        print(f"[ok] version-check {version} ({len(found)} sites)")
        return
    text_toml, text_lock = write(version, text_toml, text_lock)
    for path, text in ((toml_path, text_toml), (lock_path, text_lock)):
        staged = path.with_name(f".{path.name}.version-sync")
        try:
            staged.write_text(text, encoding="utf-8", newline="\n")
            os.replace(staged, path)
        finally:
            staged.unlink(missing_ok=True)
    print(f"[ok] synced Cargo.toml and Cargo.lock to {version}")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, tomllib.TOMLDecodeError) as error:
        sys.exit(f"error: {error}")
