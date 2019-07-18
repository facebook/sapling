# Copyright 2019 Facebook, Inc.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import ast
import atexit
import os
import sys

from .argspans import argspans


def eq(actual, expected, nested=0, eqfunc=None):
    """Check actual == expected.

    If autofix is True, record the failure and try to fix it automatically.
    If normalize is not None, and eqfunc(actual, expected) is True, treat
    actual as equal to expected. This can be used for advanced matching, for
    example, "(glob)" support.

    For autofix to work, this function has to be called using its direct name
    (ex. `eq`), without modules (ex. `testutil.eq`).

    For nested use-cases (ex. having a `fooeq(...)` function that auto fixes
    the last argument of `fooeq`. Set `nested` to the number of functions
    wrapping this function.
    """
    # For strings. Remove leading spaces and compare them again.
    if isinstance(actual, str) and isinstance(expected, str) and "\n" in expected:
        multiline = True
        actual = _removeindent(actual.replace("\t", " ")).strip()
        expected = _removeindent(expected.replace("\t", " ")).strip()
    else:
        multiline = False

    if actual == expected:
        return
    elif eqfunc is not None:
        try:
            if eqfunc(actual, expected):
                return
        except Exception:
            pass

    if autofix:
        path, lineno, indent, spans = argspans(nested)
        # Use ast.literal_eval to make sure the code evaluates to the actual
        # value. It can be `eval`, but less safe.
        if spans is not None and ast.literal_eval(repr(actual)) == actual:
            code = _repr(actual, indent + 4)
            if path not in _fixes:
                _fixes[path] = []
            # Assuming the last argument is "expected" and needs change.
            # (if nested is not 0, the "callsite" might be calling other
            # functions that take a different number of arguments).
            _fixes[path].append((spans[-1], code))
            return
        else:
            sys.stderr.write(
                "Cannot auto-fix %r => %r at %s:%s\n"
                % (expected, actual, os.path.basename(path), lineno)
            )
    else:
        if multiline:
            # Show the diff of multi-line content.
            import difflib

            diff = "".join(
                difflib.unified_diff(
                    (expected + "\n").splitlines(True),
                    (actual + "\n").splitlines(True),
                    "expected",
                    "actual",
                )
            )
            raise AssertionError("actual != expected\n%s" % diff)
        else:
            raise AssertionError("%r != %r" % (actual, expected))


def _repr(x, indent=0):
    """Similar to repr, but prefer multi-line strings instead of using '\n's"""
    if isinstance(x, str) and "\n" in x:
        # Pretty-print as a docstring with the given indentation.
        quote = '"""'
        if x.endswith('"') or '"""' in x:
            quote = "'''"
        body = ""
        for line in x.splitlines(True):
            if line not in {"\n", ""} and indent:
                line = " " * indent + line
            body += line
        return "r%s\n%s%s" % (quote, body, quote)
    else:
        return repr(x)


@atexit.register
def _fix():
    """Apply code changes"""
    for path, entries in _fixes.items():
        lines = open(path, "rb").read().splitlines(True)
        for span, code in sorted(entries, reverse=True):
            # Note: line starts with 1, col starts with 0.
            startline, startcol = span[0]
            endline, endcol = span[1]

            # This is not super efficient. But it's easy to write.
            for i in range(startline, endline + 1):
                line = lines[i - 1]
                newline = ""
                if i == startline:
                    newline += "%s%s" % (line[:startcol], code)
                if i == endline:
                    newline += line[endcol:]
                lines[i - 1] = newline

            lines = "".join(lines).splitlines(True)

        with open(path, "wb") as f:
            f.write("".join(lines))


def _removeindent(text):
    if text:
        try:
            indent = min(
                len(l) - len(l.lstrip(" "))
                for l in text.splitlines()
                if l not in {"\n", ""}
            )
        except ValueError:
            pass
        else:
            text = "".join(l[indent:] for l in text.splitlines(True))
    return text


# Whether to autofix changes. This can be changed by the callsite.
# By default, it's set if `--fix` is in sys.argv. This is convenient
# for ad-hoc runs like `python test-foo.py --fix`.
autofix = "--fix" in sys.argv

_fixes = {}  # {path: [(span, code)]}
