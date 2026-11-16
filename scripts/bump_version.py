"""Universal Version Synchronization Tool for Wintil.
Propagates version changes across all project files:
  - Cargo.toml
  - Cargo.lock
  - docs/CHANGELOG.md
  - README.md

Usage:
  python scripts/bump_version.py <new_version|patch|minor|major> [--build] [--test] [--reload]
  python scripts/bump_version.py check
"""

from __future__ import annotations

import argparse
import os
import re
import subprocess
import sys
from datetime import date
from pathlib import Path
from typing import Dict, List, Optional, Tuple

REPO_ROOT = Path(__file__).resolve().parent.parent


def write_text_atomic(path: Path, content: str) -> None:
    path.write_text(content, encoding="utf-8")


def get_current_version() -> str:
    cargo_toml = REPO_ROOT / "Cargo.toml"
    content = cargo_toml.read_text(encoding="utf-8")
    match = re.search(r'\[package\][\s\S]*?version\s*=\s*["\']([^"\']+)["\']', content)
    if not match:
        raise ValueError(f"Could not find [package] version in {cargo_toml}")
    return match.group(1).strip()


def parse_semver(ver: str) -> Tuple[int, int, int]:
    clean = ver.lstrip("v").strip()
    parts = clean.split(".")
    if len(parts) != 3 or not all(p.isdigit() for p in parts):
        raise ValueError(f"Invalid SemVer string '{ver}'. Expected format: X.Y.Z")
    return int(parts[0]), int(parts[1]), int(parts[2])


def compute_bump(current: str, bump_type: str) -> str:
    major, minor, patch = parse_semver(current)
    if bump_type == "patch":
        return f"{major}.{minor}.{patch + 1}"
    elif bump_type == "minor":
        return f"{major}.{minor + 1}.0"
    elif bump_type == "major":
        return f"{major + 1}.0.0"
    else:
        # Explicit version string supplied
        clean = bump_type.lstrip("v").strip()
        parse_semver(clean)  # Validate format
        return clean


def check_version_consistency() -> Dict[str, Optional[str]]:
    """Inspect all project metadata files and check if versions match."""
    status: Dict[str, Optional[str]] = {}

    # 1. Cargo.toml
    cargo_toml = REPO_ROOT / "Cargo.toml"
    if cargo_toml.exists():
        match = re.search(
            r'\[package\][\s\S]*?version\s*=\s*["\']([^"\']+)["\']',
            cargo_toml.read_text(encoding="utf-8"),
        )
        status["Cargo.toml"] = match.group(1) if match else None
    else:
        status["Cargo.toml"] = None

    # 2. Cargo.lock
    cargo_lock = REPO_ROOT / "Cargo.lock"
    if cargo_lock.exists():
        match = re.search(
            r'\[\[package\]\]\s*name\s*=\s*"wintil"\s*version\s*=\s*"([^"]+)"',
            cargo_lock.read_text(encoding="utf-8"),
        )
        status["Cargo.lock"] = match.group(1) if match else None
    else:
        status["Cargo.lock"] = None

    # 3. docs/CHANGELOG.md (latest version entry)
    changelog = REPO_ROOT / "docs" / "CHANGELOG.md"
    if changelog.exists():
        match = re.search(r"##\s*\[([^\]]+)\]", changelog.read_text(encoding="utf-8"))
        status["docs/CHANGELOG.md"] = match.group(1) if match else None
    else:
        status["docs/CHANGELOG.md"] = None

    return status


def bump_version(
    target: str,
    build: bool = False,
    run_tests: bool = False,
    reload_service: bool = False,
) -> None:
    old_version = get_current_version()
    new_version = compute_bump(old_version, target)

    if old_version == new_version:
        print(f"[*] Version is already {new_version}. No changes needed.")
        return

    print(f"[*] Bumping Wintil version: v{old_version} -> v{new_version}")

    updated_files: List[str] = []

    # 1. Update Cargo.toml
    cargo_toml = REPO_ROOT / "Cargo.toml"
    old_cargo = cargo_toml.read_text(encoding="utf-8")
    new_cargo = re.sub(
        r'(\[package\][\s\S]*?version\s*=\s*["\'])[^"\']+(["\'])',
        rf"\g<1>{new_version}\g<2>",
        old_cargo,
        count=1,
    )
    if new_cargo != old_cargo:
        write_text_atomic(cargo_toml, new_cargo)
        updated_files.append("Cargo.toml")

    # 2. Update Cargo.lock via cargo check
    try:
        subprocess.run(
            ["cargo", "check", "--manifest-path", str(cargo_toml)],
            cwd=str(REPO_ROOT),
            check=True,
            capture_output=True,
            text=True,
        )
        updated_files.append("Cargo.lock")
    except Exception as e:
        print(f"[!] Warning: Failed to refresh Cargo.lock via cargo check: {e}", file=sys.stderr)

    # 3. Update docs/CHANGELOG.md header if new release
    changelog = REPO_ROOT / "docs" / "CHANGELOG.md"
    if changelog.exists():
        old_cl = changelog.read_text(encoding="utf-8")
        if f"## [{new_version}]" not in old_cl:
            today_str = date.today().isoformat()
            new_entry = (
                f"## [{new_version}] - {today_str}\n\n"
                f"### Changed\n"
                f"- Version increment to {new_version}.\n\n"
                f"---\n\n"
            )
            # Insert right after the top header separator
            idx = old_cl.find("---\n\n")
            if idx != -1:
                insert_pos = idx + len("---\n\n")
                new_cl = old_cl[:insert_pos] + new_entry + old_cl[insert_pos:]
                write_text_atomic(changelog, new_cl)
                updated_files.append("docs/CHANGELOG.md")

    print(f"\n[SUCCESS] Manifest version update to v{new_version} complete! Updated:")
    for f in updated_files:
        print(f"  - {f}")

    # 4. Optional: Run tests
    if run_tests:
        print(f"\n[*] Running Cargo test suite...")
        subprocess.run(
            ["cargo", "test", "--manifest-path", str(cargo_toml)],
            cwd=str(REPO_ROOT),
            check=True,
        )
        print("[SUCCESS] All tests passed!")

    # 5. Optional: Build release binary
    if build:
        print(f"\n[*] Compiling optimized release binary (v{new_version})...")
        if reload_service:
            # End running task before building so file is not locked
            subprocess.run(["schtasks.exe", "/End", "/TN", "Wintil"], capture_output=True)

        subprocess.run(
            ["cargo", "build", "--release", "--manifest-path", str(cargo_toml)],
            cwd=str(REPO_ROOT),
            check=True,
        )
        print(f"[SUCCESS] Standalone release binary built: target/release/wintil.exe")

    # 6. Optional: Reload service
    if reload_service:
        print(f"\n[*] Reloading Wintil Task Scheduler task...")
        subprocess.run(["schtasks.exe", "/End", "/TN", "Wintil"], capture_output=True)
        subprocess.run(["schtasks.exe", "/Run", "/TN", "Wintil"], check=True)
        print("[SUCCESS] Wintil service reloaded and active!")


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Unified Version Synchronization Tool for Wintil",
    )
    parser.add_argument(
        "target",
        nargs="?",
        default="check",
        help="Target version (e.g. '0.2.0') or bump kind: 'patch', 'minor', 'major', or 'check'",
    )
    parser.add_argument(
        "--build",
        action="store_true",
        help="Compile optimized release binary after bumping",
    )
    parser.add_argument(
        "--test",
        action="store_true",
        help="Run cargo test suite",
    )
    parser.add_argument(
        "--reload",
        action="store_true",
        help="Restart Task Scheduler task 'Wintil' with newly compiled binary",
    )

    args = parser.parse_args()

    if args.target == "check":
        current = get_current_version()
        print(f"Canonical Package Version: v{current}")
        print("\nConsistency Check across metadata files:")
        status = check_version_consistency()
        all_match = True
        for file_path, ver in status.items():
            matches = ver == current
            mark = "OK" if matches else "MISMATCH"
            print(f"  [{mark:^8}] {file_path:25} -> {ver or 'MISSING'}")
            if not matches:
                all_match = False

        if all_match:
            print("\n[SUCCESS] All project version references are fully synchronized!")
        else:
            print("\n[!] Discrepancies detected. Run: python scripts/bump_version.py <version>")
            sys.exit(1)
    else:
        bump_version(
            args.target,
            build=args.build,
            run_tests=args.test,
            reload_service=args.reload,
        )


if __name__ == "__main__":
    main()
