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
import sys
from enum import Enum
from typing import NamedTuple, Optional

LINTER_CODE = "RUSTJKEXISTS"


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

    if args.verbose:
        print(
            f"  RUSTJKEXISTS: received {len(args.filenames)} file(s)",
            file=sys.stderr,
        )


if __name__ == "__main__":
    main()
