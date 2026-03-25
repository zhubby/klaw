#!/usr/bin/env python3

import argparse
import shutil
import subprocess
from pathlib import Path

from bump_root_cargo_version import prepare_workspace_version_update, validate_version


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Run the main-branch GitHub release workflow for the root Cargo.toml."
    )
    parser.add_argument(
        "--version",
        help="Explicit target version in x.y.z format. Defaults to patch + 1.",
    )
    parser.add_argument(
        "--cargo-file",
        default="Cargo.toml",
        help="Path to the root Cargo.toml file. Defaults to Cargo.toml.",
    )
    parser.add_argument(
        "--remote",
        default="origin",
        help="Git remote used for pushes. Defaults to origin.",
    )
    parser.add_argument(
        "--skip-remote-check",
        action="store_true",
        help="Skip fetching and comparing remote main. Intended for testing only.",
    )
    parser.add_argument(
        "--skip-push",
        action="store_true",
        help="Skip pushing the branch and tag. Intended for testing only.",
    )
    parser.add_argument(
        "--skip-release",
        action="store_true",
        help="Skip creating the GitHub release. Intended for testing only.",
    )
    return parser.parse_args()


def run(
    *args: str,
    cwd: Path,
    capture_output: bool = False,
) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        args,
        cwd=cwd,
        check=True,
        text=True,
        capture_output=capture_output,
    )


def require_command(name: str) -> None:
    if shutil.which(name) is None:
        raise RuntimeError(f"Required command not found: {name}")


def ensure_clean_worktree(repo_root: Path) -> None:
    status = run("git", "status", "--porcelain", cwd=repo_root, capture_output=True).stdout
    if status.strip():
        raise RuntimeError(
            "Working tree is not clean. Commit or stash existing changes before releasing."
        )


def ensure_main_branch(repo_root: Path) -> None:
    current_branch = run(
        "git",
        "branch",
        "--show-current",
        cwd=repo_root,
        capture_output=True,
    ).stdout.strip()
    if current_branch == "main":
        return

    run("git", "switch", "main", cwd=repo_root)
    current_branch = run(
        "git",
        "branch",
        "--show-current",
        cwd=repo_root,
        capture_output=True,
    ).stdout.strip()
    if current_branch != "main":
        raise RuntimeError("Failed to switch to main.")


def ensure_tag_absent(repo_root: Path, tag_name: str) -> None:
    existing = run("git", "tag", "--list", tag_name, cwd=repo_root, capture_output=True).stdout
    if existing.strip():
        raise RuntimeError(f"Tag already exists: {tag_name}")


def ensure_remote_exists(repo_root: Path, remote: str) -> None:
    remotes = run("git", "remote", cwd=repo_root, capture_output=True).stdout.splitlines()
    if remote not in remotes:
        raise RuntimeError(f"Git remote not found: {remote}")


def ensure_main_matches_remote(repo_root: Path, remote: str) -> None:
    ensure_remote_exists(repo_root, remote)
    run("git", "fetch", remote, "main", "--prune", cwd=repo_root)

    remote_ref = f"refs/remotes/{remote}/main"
    remote_main = run(
        "git",
        "rev-parse",
        "--verify",
        remote_ref,
        cwd=repo_root,
        capture_output=True,
    ).stdout.strip()
    local_main = run(
        "git",
        "rev-parse",
        "--verify",
        "refs/heads/main",
        cwd=repo_root,
        capture_output=True,
    ).stdout.strip()
    if local_main == remote_main:
        return

    ahead_behind = run(
        "git",
        "rev-list",
        "--left-right",
        "--count",
        f"main...{remote}/main",
        cwd=repo_root,
        capture_output=True,
    ).stdout.strip()
    ahead_str, behind_str = ahead_behind.split()
    raise RuntimeError(
        "Local main does not match "
        f"{remote}/main (ahead={ahead_str}, behind={behind_str}). "
        "Pull or reconcile main before releasing."
    )


def commit_version_bump(repo_root: Path, cargo_file: Path, version: str) -> None:
    relative_cargo_file = cargo_file.relative_to(repo_root)
    run("git", "add", str(relative_cargo_file), cwd=repo_root)
    run(
        "git",
        "commit",
        "-m",
        f"chore(release): cut v{version}",
        cwd=repo_root,
    )


def push_release(repo_root: Path, remote: str, tag_name: str) -> None:
    run("git", "push", remote, "main", cwd=repo_root)
    run("git", "push", remote, tag_name, cwd=repo_root)


def create_release(repo_root: Path, version: str, tag_name: str) -> str:
    title = f"release-v{version}"
    result = run(
        "gh",
        "release",
        "create",
        tag_name,
        "--title",
        title,
        "--generate-notes",
        cwd=repo_root,
        capture_output=True,
    )
    return result.stdout.strip()


def resolve_repo_root(start: Path) -> Path:
    result = run("git", "rev-parse", "--show-toplevel", cwd=start, capture_output=True)
    return Path(result.stdout.strip())


def write_version_update(cargo_file: Path, requested_version: str | None) -> tuple[str, str]:
    original_contents = cargo_file.read_text(encoding="utf-8")
    old_version, new_version, updated_contents = prepare_workspace_version_update(
        original_contents,
        requested_version,
    )
    cargo_file.write_text(updated_contents, encoding="utf-8")
    return old_version, new_version


def main() -> int:
    args = parse_args()
    if args.version is not None:
        validate_version(args.version)

    require_command("git")
    if not args.skip_release:
        require_command("gh")

    repo_root = resolve_repo_root(Path.cwd())
    cargo_file = (repo_root / args.cargo_file).resolve()

    ensure_clean_worktree(repo_root)
    ensure_main_branch(repo_root)
    ensure_clean_worktree(repo_root)
    if not args.skip_remote_check:
        ensure_main_matches_remote(repo_root, args.remote)

    original_contents = cargo_file.read_text(encoding="utf-8")
    old_version, new_version, _ = prepare_workspace_version_update(
        original_contents,
        args.version,
    )
    tag_name = f"v{new_version}"
    ensure_tag_absent(repo_root, tag_name)

    committed = False
    try:
        write_version_update(cargo_file, args.version)
        commit_version_bump(repo_root, cargo_file, new_version)
        committed = True
        run("git", "tag", "-a", tag_name, "-m", tag_name, cwd=repo_root)

        if not args.skip_push:
            push_release(repo_root, args.remote, tag_name)

        release_url = ""
        if not args.skip_release:
            release_url = create_release(repo_root, new_version, tag_name)
    except Exception:
        if not committed and cargo_file.exists():
            cargo_file.write_text(original_contents, encoding="utf-8")
        raise

    commit_sha = run("git", "rev-parse", "HEAD", cwd=repo_root, capture_output=True).stdout.strip()

    print(f"old_version={old_version}")
    print(f"new_version={new_version}")
    print(f"tag={tag_name}")
    print(f"commit={commit_sha}")
    if release_url:
        print(f"release_url={release_url}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
