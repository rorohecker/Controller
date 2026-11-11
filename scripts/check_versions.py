"""Fail unless Cargo.toml and tauri.conf.json carry the same version (and match a tag, if given).

Usage: python scripts/check_versions.py [--tag v0.22.0]
"""
import argparse
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def cargo_version() -> str:
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    package = re.search(r"^\[package\](.*?)(?=^\[|\Z)", text, re.S | re.M)
    match = re.search(r'^version\s*=\s*"([^"]+)"', package.group(1) if package else "", re.M)
    if not match:
        raise SystemExit("Cargo.toml has no [package] version")
    return match.group(1)


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("--tag", help="release tag such as v0.22.0")
    args = parser.parse_args()
    cargo = cargo_version()
    tauri = json.loads((ROOT / "tauri.conf.json").read_text(encoding="utf-8"))["version"]
    problems = []
    if cargo != tauri:
        problems.append(f"Cargo.toml is {cargo} but tauri.conf.json is {tauri}")
    if args.tag and args.tag != f"v{tauri}":
        problems.append(f"tag {args.tag} does not match version v{tauri}")
    if problems:
        print("Version check failed:\n  " + "\n  ".join(problems), file=sys.stderr)
        return 1
    print(f"Versions agree: {cargo}" + (f" (tag {args.tag})" if args.tag else ""))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
