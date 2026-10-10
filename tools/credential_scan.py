#!/usr/bin/env python3
"""Scan repository and workspace for credential-shaped files or content."""

import argparse
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]

SECRET_PATTERNS = (
    ("github_pat", re.compile(rb"github_pat_[A-Za-z0-9_]{20,}")),
    ("github_token", re.compile(rb"ghp_[A-Za-z0-9]{20,}")),
    ("private_key", re.compile(
        rb"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----"
    )),
)

CONFIG_PASSWORD_PATTERN = re.compile(rb'"account_password"\s*:\s*"[^"]+"')

SENSITIVE_NAME_PATTERNS = (
    re.compile(r"^testaccount.*\.txt$", re.IGNORECASE),
    re.compile(r"^githubtoken", re.IGNORECASE),
    re.compile(r"^kindle_key(?:\.pub)?$", re.IGNORECASE),
    re.compile(r"^id_(?:rsa|ed25519)$", re.IGNORECASE),
    re.compile(r"^\.env(?:\..+)?$", re.IGNORECASE),
    re.compile(r"\.(?:pem|key)$", re.IGNORECASE),
)

SKIP_DIR_NAMES = {
    ".git",
    ".venv",
    "__pycache__",
    "build",
    "cache",
    "logs",
    "node_modules",
    "refs",
}


def is_sensitive_name(path):
    return any(pattern.search(path.name) for pattern in SENSITIVE_NAME_PATTERNS)


def scan_file(path):
    findings = []
    try:
        data = path.read_bytes()
    except OSError as exc:
        return [(path, "unreadable: %s" % exc)]
    for label, pattern in SECRET_PATTERNS:
        if pattern.search(data):
            findings.append((path, label))
    if path.name == "config.json" and path.parent.name == "bin":
        if CONFIG_PASSWORD_PATTERN.search(data):
            findings.append((path, "non-empty account_password"))
    return findings


def repository_files(root):
    try:
        completed = subprocess.run(
            ["git", "-C", str(root), "ls-files", "-z"],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
        )
    except (OSError, subprocess.CalledProcessError):
        return [
            path for path in root.rglob("*")
            if path.is_file() and not any(part in SKIP_DIR_NAMES for part in path.parts)
        ]
    names = completed.stdout.decode("utf-8", "surrogateescape").split("\0")
    # 子模块 (rust/third_party/FBInk) 在 ls-files 里是目录, 跳过
    return [root / name for name in names if name and (root / name).is_file()]


def scan_repository(root):
    findings = []
    for path in repository_files(root):
        if is_sensitive_name(path):
            findings.append((path, "sensitive filename"))
        findings.extend(scan_file(path))
    return findings


def scan_workspace(workspace, repository=None, max_depth=3):
    findings = []
    workspace = Path(workspace).resolve()
    repository = Path(repository).resolve() if repository else None
    for path in workspace.rglob("*"):
        try:
            relative = path.relative_to(workspace)
        except ValueError:
            continue
        if len(relative.parts) > max_depth:
            continue
        if any(part in SKIP_DIR_NAMES for part in relative.parts):
            continue
        if repository is not None:
            try:
                path.resolve().relative_to(repository)
                continue
            except ValueError:
                pass
        if path.is_file() and is_sensitive_name(path):
            findings.append((path, "sensitive filename outside repository"))
    return findings


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", default=str(ROOT))
    parser.add_argument("--workspace")
    parser.add_argument("--max-depth", type=int, default=3)
    args = parser.parse_args(argv)

    root = Path(args.root).resolve()
    findings = scan_repository(root)
    workspace = Path(args.workspace).resolve() if args.workspace else None
    if workspace is not None:
        findings.extend(scan_workspace(workspace, root, args.max_depth))

    if findings:
        for path, reason in findings:
            print("%s: %s" % (path, reason))
        return 1
    print("credential scan clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())
