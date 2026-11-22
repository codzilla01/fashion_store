#!/usr/bin/env python3
"""Verify that a release tag matches every source-of-truth version.

The release workflow calls this before building anything so a tag cannot
produce an installer whose internal version differs from its filename.
"""
from __future__ import annotations

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def read_json(path: Path) -> dict:
    with path.open(encoding="utf-8") as handle:
        return json.load(handle)


def main() -> int:
    if len(sys.argv) != 2:
        print("usage: check_release_version.py <version>", file=sys.stderr)
        return 2
    version = sys.argv[1].strip()
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        print(f"::error::Invalid release version {version!r}; expected X.Y.Z")
        return 1

    sources = {
        "apps/pos-desktop/package.json": read_json(ROOT / "apps/pos-desktop/package.json").get("version"),
        "apps/pos-desktop/src-tauri/tauri.conf.json": read_json(ROOT / "apps/pos-desktop/src-tauri/tauri.conf.json").get("version"),
        "apps/activation-tool/package.json": read_json(ROOT / "apps/activation-tool/package.json").get("version"),
        "apps/activation-tool/src-tauri/tauri.conf.json": read_json(ROOT / "apps/activation-tool/src-tauri/tauri.conf.json").get("version"),
    }
    bad = {path: actual for path, actual in sources.items() if actual != version}
    if bad:
        for path, actual in bad.items():
            print(f"::error::{path} has version {actual!r}, expected {version!r}")
        return 1
    print(f"Release version {version} is consistent across POS and Activation Tool.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
