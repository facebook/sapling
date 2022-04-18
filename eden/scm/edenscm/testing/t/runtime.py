# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""runtime for python code transformed from a '.t' test"""

import fnmatch
import os
import re


def eqglob(a: str, b: str) -> bool:
    r"""compare multi-line strings, with '(glob)', '(re)', '(esc)' support

    >>> eqglob("a\na\n", "a\na\n")
    True
    >>> eqglob("a\na\n", "a\nb\n")
    False
    >>> eqglob("a\na\n", "[ab] (re)\n* (glob)\n")
    True
    >>> eqglob("c\n", "[ab] (re)")
    False
    """
    if not (isinstance(a, str) and isinstance(b, str)):
        return False
    alines = a.splitlines()
    blines = b.splitlines()
    if len(alines) != len(blines):
        return False
    for aline, bline in zip(alines, blines):
        if bline.endswith(" (esc)"):
            # If it's a unicode string that contains escapes, turn it to binary
            # first.
            bline = bline[:-6].encode("raw_unicode_escape").decode("unicode-escape")
        if os.name == "nt":
            # Normalize path on Windows.
            aline = aline.replace("\\", "/")
            bline = bline.replace("\\", "/")
        if bline.endswith(" (glob)"):
            # As an approximation, use fnmatch to do the job.
            # "[]" do not have special meaning in run-tests.py glob patterns.
            # Replace them with "?".
            globline = bline[:-7].replace("[", "?").replace("]", "?")
            if not fnmatch.fnmatch(aline, globline):
                return False
        elif bline.endswith(" (re)"):
            if not re.match(bline[:-5] + r"\Z", aline):
                return False
        elif aline != bline:
            return False
    return True
