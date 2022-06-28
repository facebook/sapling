# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import os
import sys
from unittest import SkipTest

from .t.runner import fixmismatches, runtest, TestId

DESCRIPTION = """single .t test runner for run-tests.py integration

This entry point runs a single test using features in the new testing
module. It is intended to be run via 'run-tests.py' for easy integration.
If you only need to use the new test runner, use 'debugruntest' instead.

Unlike 'debugruntest', this entry point only runs a single test, and:
- Does not write mismatch to stdio. Write "fixed" output to --output path.
- Does not maintain a test process pool. Expects run-tests.py to do so.
- Does not spawn child processes (using multiprocessing) for clean environment.

Exit code:
- 0: Test passed
- 1: Test failed (output mismatch or exception)
- 80: Test skipped
"""


def main():
    parser = argparse.ArgumentParser(description=DESCRIPTION)
    parser.add_argument(
        "-o",
        "--output",
        help="write test output to the given file",
        type=str,
    )
    parser.add_argument(
        "-e",
        "--ext",
        help="testing extension to enable",
        action="append",
        default=["edenscm.testing.ext.hg", "edenscm.testing.ext.python"],
    )
    parser.add_argument("path", metavar="PATH", type=str, help="test file path")
    args = parser.parse_args()

    testid = TestId.frompath(args.path)
    exts = args.ext
    outpath = args.output

    mismatches = []

    def mismatchcb(mismatch, outpath=outpath):
        if outpath:
            mismatch.filename = outpath
        mismatches.append(mismatch)

    if outpath:
        try:
            os.unlink(outpath)
        except FileNotFoundError:
            pass

    try:
        runtest(testid, exts, mismatchcb)
    except SkipTest:
        return 80
    except Exception:
        raise
    finally:
        if not mismatches:
            return 0
        if outpath:
            # fix mismatches on outpath
            with open(testid.path, "rb") as src, open(outpath, "wb") as dst:
                dst.write(src.read())
            fixmismatches(mismatches)
        return 1


if __name__ == "__main__":
    sys.exit(main())
