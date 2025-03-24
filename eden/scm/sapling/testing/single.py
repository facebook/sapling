# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

import argparse
import logging
import os
import sys
from unittest import SkipTest

from .t.runner import fixmismatches, runtest, runtest_reporting_progress, TestId

SKIP_PYTHON_LOOKUP = True

DESCRIPTION = """Entry point to run a single .t test.

Support 2 modes:

## `--output` mode

`--output`: used by the old `run-tests.py`, write the "fixed" test file
(usually, `test-x.t.err`) so `run-tests.py` can diff them. Mismatches won't be
reported until the end of the test.

Exit code (matches run-tests.py DebugRunTestTest):
- 0: Test passed
- 1: Test failed (output mismatch)
- 80: Test skipped
- 81: Test failed (Python exception)

## `--structured-output` mode

`--structured-output`: used by the new `sl .t` command, report output
mismatches as they are discovered.

Exit code is 0. Test result should be reported to the specified file.
"""

logger = logging.getLogger(__name__)


def main():
    # argparse does not like "None" argv[0], which can happen with the
    # builtin module importer.
    if sys.argv[0] is None:
        sys.argv = [""] + list(sys.argv[1:])
    parser = argparse.ArgumentParser(
        description=DESCRIPTION, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "-o",
        "--output",
        help="write test output to the given file",
        type=str,
    )
    parser.add_argument(
        "-p",
        "--structured-output",
        help="write structured output (JSON per line) to the given file",
        type=str,
    )
    parser.add_argument(
        "-e",
        "--ext",
        help="testing extension to enable",
        action="append",
        default=[],
    )
    parser.add_argument(
        "-d",
        "--debug",
        action="store_true",
        help="enable debug logging",
    )
    parser.add_argument(
        "-N",
        "--no-default-exts",
        action="store_true",
        help="disable default extensions",
    )
    parser.add_argument("path", metavar="PATH", type=str, help="test file path")
    args = parser.parse_args()

    log_level = logging.DEBUG if args.debug else logging.INFO
    logging.basicConfig(level=log_level)

    testid = TestId.frompath(args.path)
    default_exts = (
        []
        if args.no_default_exts
        else ["sapling.testing.ext.hg", "sapling.testing.ext.python"]
    )
    exts = default_exts + args.ext
    outpath = args.output
    structured_output = args.structured_output
    if structured_output:
        if outpath:
            raise ValueError("--structured-output conflicts with --output")
        return runtest_reporting_progress(testid, exts, structured_output)
    else:
        return runtest_with_output(testid, exts, outpath)


def runtest_with_output(testid, exts, outpath):
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
    except SkipTest as e:
        logger.debug(e)
        return 80
    if not mismatches:
        return 0
    if outpath:
        # fix mismatches on outpath
        with open(testid.path, "rb") as src, open(outpath, "wb") as dst:
            dst.write(src.read())
        fixmismatches(mismatches)
    return 1


if __name__ == "__main__":
    exitcode = 0
    try:
        exitcode = main()
    except BaseException:
        import traceback

        traceback.print_exc()
        exitcode = 81
    sys.exit(exitcode)
