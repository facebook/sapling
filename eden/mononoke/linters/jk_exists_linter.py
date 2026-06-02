# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Lint rule: every JustKnob referenced in Mononoke Rust source files must exist.

If a knob name is misspelled or references a knob that was never created,
the Rust JK API returns an error at runtime, which can crash the service
if unwrapped. This linter catches non-existent JK references at diff time
so they cannot land.

Uses regex-based extraction of JK string literals from Rust code, then
validates existence via the ``jk`` CLI.

Output: newline-delimited JSON (LintMessage schema) on stdout.
"""

from __future__ import annotations

import argparse
import json
import re
import shutil
import subprocess
import sys
from collections import defaultdict
from enum import Enum
from typing import NamedTuple, Optional

LINTER_CODE = "RUSTJKEXISTS"
JK_QUERY_TIMEOUT = 30

# Matches any string literal that looks like a JK name ("knobset/path:knob").
# Broader than matching only justknobs:: call sites -- also catches references
# via const declarations, struct fields, and enum variant construction.
JK_STRING_RE = re.compile(
    r'"([a-z][a-z0-9_]*(?:/[a-z][a-z0-9_]*)+:[a-z][a-z0-9_:]*[a-z0-9_])"'
)


class LintSeverity(str, Enum):
    ERROR = "error"
    WARNING = "warning"
    ADVICE = "advice"
    DISABLED = "disabled"


class LintMessage(NamedTuple):
    path: str
    line: Optional[int]
    char: Optional[int]
    code: str
    severity: LintSeverity
    name: str
    original: Optional[str] = None
    replacement: Optional[str] = None
    description: Optional[str] = None
    bypassChangedLineFiltering: Optional[bool] = None
    failureCategory: Optional[str] = None


def extract_jk_references_regex(
    filepaths: list[str],
    verbose: bool,
) -> list[tuple[str, int, str]]:
    """Extract JK references from Rust files using regex matching.

    Returns list of (filepath, line_number, jk_name).
    """
    results: list[tuple[str, int, str]] = []
    seen: set[tuple[str, int, str]] = set()

    for filepath in filepaths:
        try:
            with open(filepath) as f:
                content = f.read()
        except OSError:
            continue

        for m in JK_STRING_RE.finditer(content):
            jk_name = m.group(1)
            line_no = content[: m.start()].count("\n") + 1
            key = (filepath, line_no, jk_name)
            if key not in seen:
                seen.add(key)
                results.append(key)
                if verbose:
                    print(
                        f"  [{filepath}:{line_no}] found JK: {jk_name}",
                        file=sys.stderr,
                    )

    return results


def _parse_jk_knobset(jk_name: str) -> str:
    """Extract the knobset from a JK name (everything before the first colon)."""
    return jk_name.split(":", 1)[0]


def _query_knobset_knobs(
    knobset: str,
    verbose: bool,
    jk_command: str = "jk",
) -> set[str]:
    """Query all knob names in a knobset via ``jk get <knobset>``.

    Returns a set of fully-qualified knob names.
    Raises RuntimeError if the command fails.
    """
    cmd = [jk_command, "get", knobset]

    if verbose:
        print(f"  Running: jk get {knobset}", file=sys.stderr)

    result = subprocess.run(
        cmd,
        capture_output=True,
        text=True,
        timeout=JK_QUERY_TIMEOUT,
    )

    if result.returncode != 0:
        raise RuntimeError(
            f"jk get {knobset} returned exit code {result.returncode}: "
            f"{result.stderr.strip()}"
        )

    knobs: set[str] = set()
    prefix = f"{knobset}:"
    for line in result.stdout.splitlines():
        line = line.strip()
        if line.startswith(prefix):
            metadata_sep = " (last modified: "
            if metadata_sep in line:
                line = line.split(metadata_sep, 1)[0]
            last_sep = line.rfind(": ")
            if last_sep > 0:
                knob_name = line[:last_sep]
                knobs.add(knob_name)

    if verbose:
        print(
            f"  Found {len(knobs)} knobs in knobset {knobset}",
            file=sys.stderr,
        )

    return knobs


def query_jk_existence(
    jk_names: list[str],
    verbose: bool,
    jk_command: str = "jk",
) -> dict[str, bool]:
    """Check whether each JK name exists by querying knobsets in batches.

    Returns a dict mapping each JK name to True (exists) or False (not found).
    On per-knobset query failure, assumes knobs exist (safe default).
    """
    if not jk_names:
        return {}

    by_knobset: dict[str, list[str]] = defaultdict(list)
    for name in jk_names:
        ks = _parse_jk_knobset(name)
        by_knobset[ks].append(name)

    if verbose:
        print(
            f"  Querying {len(by_knobset)} knobset(s): "
            f"{', '.join(sorted(by_knobset.keys()))}",
            file=sys.stderr,
        )

    existence: dict[str, bool] = {}
    for ks, names in by_knobset.items():
        try:
            known_knobs = _query_knobset_knobs(ks, verbose, jk_command)
        except (RuntimeError, subprocess.TimeoutExpired) as e:
            if verbose:
                print(f"  Skipping knobset {ks}: {e}", file=sys.stderr)
            for name in names:
                existence[name] = True
            continue
        for name in names:
            existence[name] = name in known_knobs

    return existence


def emit(msg: LintMessage) -> None:
    print(json.dumps(msg._asdict()), flush=True)


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Check that JustKnobs used in Mononoke Rust code actually exist.",
        fromfile_prefix_chars="@",
    )
    parser.add_argument(
        "filenames",
        nargs="+",
        help="Rust source files to check.",
    )
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Print diagnostic info to stderr.",
    )
    parser.add_argument(
        "--severity",
        default="error",
        choices=["error", "warning", "advice"],
        help="Severity for non-existent JustKnobs (default: error).",
    )
    parser.add_argument(
        "--jk-command",
        default="jk",
        help="Path to the jk CLI binary (default: jk).",
    )
    args = parser.parse_args()

    severity = LintSeverity(args.severity)

    all_refs = extract_jk_references_regex(args.filenames, args.verbose)
    unique_jks = {jk_name for _, _, jk_name in all_refs}

    if not unique_jks:
        if args.verbose:
            print("  No JK references found in any files.", file=sys.stderr)
        return

    jk_command = args.jk_command
    if shutil.which(jk_command) is None:
        emit(
            LintMessage(
                path=args.filenames[0],
                line=None,
                char=None,
                code=LINTER_CODE,
                severity=LintSeverity.DISABLED,
                name="jk-not-available",
                description=(
                    "The 'jk' CLI is not available. Cannot verify JustKnob existence."
                ),
                failureCategory="skipped",
            )
        )
        return

    if args.verbose:
        print(
            f"  Found {len(all_refs)} JK references "
            f"({len(unique_jks)} unique) across {len(args.filenames)} files.",
            file=sys.stderr,
        )

    existence = query_jk_existence(list(unique_jks), args.verbose, jk_command)

    for filepath, line_no, jk_name in all_refs:
        exists = existence.get(jk_name, True)

        if not exists:
            ks = _parse_jk_knobset(jk_name)
            emit(
                LintMessage(
                    path=filepath,
                    line=line_no,
                    char=1,
                    code=LINTER_CODE,
                    severity=severity,
                    name="jk-not-found",
                    description=(
                        f"JustKnob '{jk_name}' does not exist in knobset "
                        f"'{ks}'. This will cause a runtime error and may "
                        f"crash the service. Create the knob first at "
                        f"https://www.internalfb.com/justknobs/{ks}"
                    ),
                )
            )
        elif args.verbose:
            print(
                f"  OK: {jk_name} exists.",
                file=sys.stderr,
            )


if __name__ == "__main__":
    main()
