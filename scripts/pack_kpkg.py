#!/usr/bin/env python3
"""Pack a KPM package directory into a .kpkg tar archive."""

from __future__ import annotations

import argparse
import json
import os
import sys
import tarfile


RESERVED = {"rootfs", "startup.sh"}


def main() -> int:
    parser = argparse.ArgumentParser(description="Pack a KPM package folder into a .kpkg file")
    parser.add_argument("pkg_path", help="Path to the package folder")
    parser.add_argument("output_path", help="Output .kpkg file or directory")
    args = parser.parse_args()

    manifest_path = os.path.join(args.pkg_path, "manifest.json")
    if not os.path.isfile(manifest_path):
        print("[ERR] manifest.json file not found", file=sys.stderr)
        return 1

    with open(manifest_path, encoding="utf-8") as file:
        manifest = json.load(file)

    version = ".".join(str(part) for part in manifest["version"])
    platforms = "-".join(manifest.get("supported_platforms", ["kindleany"]))
    filename = f"{manifest['id']}_{version}_{platforms}.kpkg"
    output = args.output_path
    if os.path.isdir(output):
        output = os.path.join(output, filename)

    for name in os.listdir(args.pkg_path):
        if name in RESERVED:
            print(f"[ERR] reserved name '{name}' found in package", file=sys.stderr)
            return 1

    print(f"ID: {manifest['id']}")
    print(f"Name: {manifest['name']}")
    print(f"Packing -> {output}")

    with tarfile.open(output, "w:") as archive:
        for name in sorted(os.listdir(args.pkg_path)):
            print(f"- {name}")
            archive.add(os.path.join(args.pkg_path, name), arcname=name)

    print("Done!")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
