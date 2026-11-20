# SPDX-License-Identifier: MIT OR Apache-2.0
"""Check the licence of every package in the resolved dependency graph.

cargo-deny checks licences, but the version this repository uses leaves out
dev-dependencies, so test-only crates would go unchecked. This check reads
the whole graph from `cargo metadata` (every target, every dependency kind)
and evaluates each package's SPDX expression against the allow list in
deny.toml, which stays the one list. Workspace members are not checked.
A package with no SPDX expression fails.

Usage: licenses-check.py [--metadata FILE] [--deny FILE]
"""

import json
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


class Malformed(Exception):
    pass


def tokens(expression):
    text = expression.replace("(", " ( ").replace(")", " ) ").replace("/", " OR ")
    return text.split()


def allowed(expression, allow):
    """Whether an SPDX expression is satisfied by the allow list: OR needs
    one branch, AND needs every branch, `X WITH Y` must be listed as such."""
    items = tokens(expression)
    position = 0

    def peek():
        return items[position] if position < len(items) else None

    def take():
        nonlocal position
        if position >= len(items):
            raise Malformed(expression)
        position += 1
        return items[position - 1]

    def factor():
        token = take()
        if token == "(":
            value = disjunction()
            if take() != ")":
                raise Malformed(expression)
            return value
        if token in ("AND", "OR", "WITH", ")"):
            raise Malformed(expression)
        if peek() == "WITH":
            take()
            exception = take()
            if exception in ("AND", "OR", "WITH", "(", ")"):
                raise Malformed(expression)
            return f"{token} WITH {exception}" in allow
        return token in allow

    def conjunction():
        value = factor()
        while peek() == "AND":
            take()
            value = factor() and value
        return value

    def disjunction():
        value = conjunction()
        while peek() == "OR":
            take()
            value = conjunction() or value
        return value

    value = disjunction()
    if position != len(items):
        raise Malformed(expression)
    return value


def main(argv):
    metadata_file = None
    deny_file = ROOT / "deny.toml"
    arguments = list(argv)
    while arguments:
        flag = arguments.pop(0)
        if flag == "--metadata" and arguments:
            metadata_file = Path(arguments.pop(0))
        elif flag == "--deny" and arguments:
            deny_file = Path(arguments.pop(0))
        else:
            print(f"usage: {Path(__file__).name} [--metadata FILE] [--deny FILE]", file=sys.stderr)
            return 2
    allow = set(tomllib.loads(deny_file.read_text(encoding="utf-8"))["licenses"]["allow"])
    if metadata_file is None:
        run = subprocess.run(
            ["cargo", "metadata", "--locked", "--format-version", "1"],
            cwd=ROOT,
            capture_output=True,
            check=False,
        )
        if run.returncode != 0:
            sys.stderr.write(run.stderr.decode(errors="replace"))
            print("licenses-check: cargo metadata failed; licences were not checked", file=sys.stderr)
            return 1
        metadata = json.loads(run.stdout)
    else:
        metadata = json.loads(metadata_file.read_text(encoding="utf-8"))
    members = set(metadata.get("workspace_members", []))
    packages = {package["id"]: package for package in metadata["packages"]}
    resolved = [node["id"] for node in metadata["resolve"]["nodes"]]
    problems = []
    for package_id in resolved:
        if package_id in members:
            continue
        package = packages[package_id]
        name = f"{package['name']} {package['version']}"
        expression = package.get("license")
        if not expression:
            problems.append(f"{name}: no SPDX licence expression")
            continue
        try:
            if not allowed(expression, allow):
                problems.append(f"{name}: {expression} is not allowed")
        except Malformed:
            problems.append(f"{name}: cannot read the licence expression {expression!r}")
    for problem in problems:
        print(f"licenses-check: {problem}", file=sys.stderr)
    if problems:
        return 1
    print(f"licenses-check: {len(resolved) - len(members & set(resolved))} packages allowed")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
