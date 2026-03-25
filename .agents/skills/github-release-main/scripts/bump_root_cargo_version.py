#!/usr/bin/env python3

import argparse
import re
from pathlib import Path


SECTION_HEADER_RE = re.compile(r"^\s*\[(?P<section>[^\]]+)\]\s*$")
VERSION_LINE_RE = re.compile(r'^(?P<indent>\s*)version\s*=\s*"(?P<version>\d+\.\d+\.\d+)"\s*$')


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Update [workspace.package].version in the root Cargo.toml."
    )
    parser.add_argument(
        "--file",
        default="Cargo.toml",
        help="Path to the root Cargo.toml file. Defaults to Cargo.toml.",
    )
    parser.add_argument(
        "--version",
        help="Explicit target version in x.y.z format. Defaults to patch + 1.",
    )
    return parser.parse_args()


def bump_patch(version: str) -> str:
    major_str, minor_str, patch_str = version.split(".")
    return f"{int(major_str)}.{int(minor_str)}.{int(patch_str) + 1}"


def validate_version(version: str) -> str:
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise ValueError(f"Invalid version '{version}'. Expected x.y.z.")
    return version


def prepare_workspace_version_update(
    contents: str,
    requested_version: str | None,
) -> tuple[str, str, str]:
    lines = contents.splitlines()
    updated_lines: list[str] = []
    in_workspace_package = False
    found_section = False
    old_version: str | None = None
    new_version: str | None = None

    for line in lines:
        header_match = SECTION_HEADER_RE.match(line)
        if header_match:
            in_workspace_package = header_match.group("section") == "workspace.package"
            found_section = found_section or in_workspace_package

        if in_workspace_package:
            version_match = VERSION_LINE_RE.match(line)
            if version_match and old_version is None:
                old_version = version_match.group("version")
                new_version = (
                    validate_version(requested_version)
                    if requested_version is not None
                    else bump_patch(old_version)
                )
                indent = version_match.group("indent")
                line = f'{indent}version = "{new_version}"'

        updated_lines.append(line)

    if not found_section:
        raise ValueError("Could not find [workspace.package] section.")
    if old_version is None or new_version is None:
        raise ValueError("Could not find version in [workspace.package].")

    updated_contents = "\n".join(updated_lines) + "\n"
    return old_version, new_version, updated_contents


def update_workspace_version(path: Path, requested_version: str | None) -> tuple[str, str]:
    original_contents = path.read_text(encoding="utf-8")
    old_version, new_version, updated_contents = prepare_workspace_version_update(
        original_contents,
        requested_version,
    )
    path.write_text(updated_contents, encoding="utf-8")
    return old_version, new_version


def main() -> int:
    args = parse_args()
    cargo_toml = Path(args.file)
    old_version, new_version = update_workspace_version(cargo_toml, args.version)
    print(f"{old_version} -> {new_version}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
