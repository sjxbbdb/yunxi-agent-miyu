#!/usr/bin/env python3
"""G0 public-tree privacy gate.

The gate scans tracked text files only. It reports categories, counts, and
file paths; it never prints matching lines or matched values. Known public
third-party signing constants are classified separately instead of being
treated as user credentials.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path


REPO = Path(__file__).resolve().parents[2]
SKIP_PATHS = {"testkit/privacy/g0_scan.py"}

# These are public client-signing constants from the upstream bilibili-live
# client. Their presence is classified, not treated as a user credential.
PUBLIC_ALLOWLIST = {
    "src/personas/default/skills/bilibili-live/scripts/bilibili_live_stream": {
        "aae92bc66f3edfab",
        "af125a0d5279fd576c1b4418a3e8276d",
    }
}

TEXT_SUFFIXES = {
    ".c",
    ".css",
    ".fish",
    ".h",
    ".html",
    ".json",
    ".js",
    ".lock",
    ".md",
    ".ps1",
    ".py",
    ".rs",
    ".sh",
    ".toml",
    ".txt",
    ".yaml",
    ".yml",
    "",
}

# These are deliberately short, synthetic values used by checked-in tests.
# They are classified separately so a test fixture does not hide a real token.
FIXTURE_ALLOWLIST = {
    "crates/yunxi-engine/src/tools/image_generation.rs": {"sk-written-mid-turn"},
    "crates/yunxi-hosts/src/render/tests/tool_summary.rs": {"ghp_super-secret"},
}

PERSONAL_PATH_PATTERNS = (
    re.compile(r"(?i)(?<![\w])[A-Z]:\\(?:Users|Documents and Settings)\\[^\r\n\"'<>|]+"),
    re.compile(r"(?i)(?<![\w])[A-Z]:\\(?:YunXi Agent|YunXi-Miyu)(?:\\[^\r\n\"'<>|]*)?"),
    re.compile(r"(?<![\w])/(?:home|Users|mnt/[A-Za-z])/[^\r\n\"'<>]+"),
)

PLACEHOLDER_PATH_PREFIXES = (
    "/home/tester",
    "/home/test",
    "/home/user",
    "/home/x",
    "/home/u",
    "/home/mac",
    "/home/me",
    "/home/other",
    "/home/a",
    "/home/rm.txt",
    "/home/napcat",
    "/home/linuxbrew",
    "/home/someone",
    "/home/proj",
    "/Users/someone",
)

def personal_path_patterns() -> tuple[re.Pattern[str], ...]:
    """Build patterns from the machine running the gate, not a hard-coded user."""

    candidates = {
        str(Path.home()),
        os.environ.get("USERPROFILE", ""),
        os.environ.get("HOME", ""),
    }
    patterns: list[re.Pattern[str]] = []
    for candidate in candidates:
        candidate = candidate.strip().rstrip("/\\")
        if not candidate or candidate in {"/", "\\"}:
            continue
        patterns.append(re.compile(re.escape(candidate), re.IGNORECASE))
    return tuple(patterns)


PRIVATE_KEY_RE = re.compile(
    r"-----BEGIN (?:[A-Z0-9 ]+ )?PRIVATE KEY-----|-----BEGIN OPENSSH PRIVATE KEY-----",
    re.IGNORECASE,
)

# Conservative token shapes. Short test fixtures such as sk-test and
# Bearer super-secret are intentionally not classified as credentials.
TOKEN_PATTERNS = (
    re.compile(r"\b(?:ghp_|gho_|github_pat_|glpat-|xox[baprs]-)[A-Za-z0-9_-]{16,}\b"),
    re.compile(r"\bsk-[A-Za-z0-9_-]{16,}\b"),
    re.compile(r"\bAKIA[0-9A-Z]{16}\b"),
    re.compile(r"\bAIza[A-Za-z0-9_-]{20,}\b"),
    re.compile(r"(?i)\bBearer\s+[A-Za-z0-9._~+/=-]{16,}\b"),
)


def tracked_files(repo: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "-C", str(repo), "ls-files", "-z"],
        check=True,
        capture_output=True,
    )
    return [
        repo / item
        for item in result.stdout.decode("utf-8").split("\0")
        if (
            item
            and item not in SKIP_PATHS
            and Path(item).suffix.lower() in TEXT_SUFFIXES
        )
    ]


def is_text(data: bytes) -> bool:
    # A NUL is a reliable boundary for this source/docs gate.
    return b"\0" not in data


def allowlisted_values(relative: str, text: str) -> set[str]:
    values = PUBLIC_ALLOWLIST.get(relative, set())
    return {value for value in values if value in text}


def fixture_values(relative: str, text: str) -> set[str]:
    values = FIXTURE_ALLOWLIST.get(relative, set())
    return {value for value in values if value in text}


def has_personal_path(text: str) -> bool:
    for pattern in PERSONAL_PATH_PATTERNS:
        for match in pattern.finditer(text):
            value = match.group(0)
            if any(value.startswith(prefix) for prefix in PLACEHOLDER_PATH_PREFIXES):
                continue
            return True
    return False


def scan_file(path: Path, repo: Path) -> tuple[set[str], set[str]]:
    """Return (finding categories, allowlist categories) without exposing text."""

    try:
        data = path.read_bytes()
    except OSError:
        return {"unreadable"}, set()
    if not is_text(data):
        return set(), set()

    text = data.decode("utf-8", errors="replace")
    relative = path.relative_to(repo).as_posix()
    allowlisted = allowlisted_values(relative, text)
    fixtures = fixture_values(relative, text)
    categories: set[str] = set()
    if has_personal_path(text) or any(pattern.search(text) for pattern in personal_path_patterns()):
        categories.add("personal_path")
    if PRIVATE_KEY_RE.search(text):
        categories.add("private_key")
    token_matches = [
        match.group(0)
        for pattern in TOKEN_PATTERNS
        for match in pattern.finditer(text)
    ]
    if any(
        not any(fixture in value for fixture in fixtures)
        for value in token_matches
    ):
        categories.add("credential_shape")
    allowlist_categories = set()
    if allowlisted:
        allowlist_categories.add("public_allowlist")
    if fixtures:
        allowlist_categories.add("fixture_allowlist")
    return categories, allowlist_categories


def scan(repo: Path) -> dict[str, object]:
    categories = {
        "personal_path": set(),
        "private_key": set(),
        "credential_shape": set(),
        "unreadable": set(),
        "public_allowlist": set(),
        "fixture_allowlist": set(),
    }
    files = tracked_files(repo)
    for path in files:
        findings, allowlist = scan_file(path, repo)
        relative = path.relative_to(repo).as_posix()
        for category in findings:
            categories[category].add(relative)
        for category in allowlist:
            categories[category].add(relative)

    blocked = {
        category: sorted(paths)
        for category, paths in categories.items()
        if category not in {"public_allowlist", "fixture_allowlist"} and paths
    }
    return {
        "status": "passed" if not blocked else "failed",
        "tracked_files": len(files),
        "findings": {
            category: {"count": len(paths), "paths": sorted(paths)}
            for category, paths in categories.items()
        },
    }


def self_test() -> None:
    cases = {
        "personal_path": (str(Path.home() / "private.txt"), "personal_path"),
        "private_key": ("-----BEGIN PRIVATE KEY-----", "private_key"),
        "long_token": ("Authorization: Bearer " + "a" * 24, "credential_shape"),
        "short_fixture": ("Authorization: Bearer super-secret", None),
    }
    for label, (sample, expected) in cases.items():
        found: set[str] = set()
        if has_personal_path(sample) or any(
            pattern.search(sample) for pattern in personal_path_patterns()
        ):
            found.add("personal_path")
        if PRIVATE_KEY_RE.search(sample):
            found.add("private_key")
        if any(pattern.search(sample) for pattern in TOKEN_PATTERNS):
            found.add("credential_shape")
        if expected is None:
            assert not found, label
        else:
            assert expected in found, label
    print(json.dumps({"status": "passed", "self_test": True}, ensure_ascii=False))


def main() -> int:
    parser = argparse.ArgumentParser(description="Run the G0 public-tree privacy gate")
    parser.add_argument("--repo", type=Path, default=REPO)
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return 0
    report = scan(args.repo.resolve())
    print(json.dumps(report, ensure_ascii=False, sort_keys=True))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    sys.exit(main())
