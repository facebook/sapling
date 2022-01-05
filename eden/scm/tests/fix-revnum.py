#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

"""
Rewrite rev numbers in tests reported by scmutil.trackrevnumfortests

Usage:
- Delete `.testrevnum`.
- Set `TRACKREVNUM`.
- Run tests to fix. Rev number usages are written in `.testrevnum`.
- Run this script.
"""

import re
import sys


contents = {}  # {path: lines}
fixed = set()

commithashre = re.compile(r"\A[0-9a-f]{6,40}\Z")

quote = repr

progress = 0


def fix(path, linenum, numstr, spec):
    """Attempt to replace numstr to spec at the given file and line"""
    # Do not fix the same thing multiple times.
    key = (path, linenum, numstr)
    if key in fixed:
        return
    else:
        fixed.add(key)

    if path not in contents:
        with open(path, "rb") as f:
            contents[path] = f.read().splitlines(True)
    lines = contents[path]
    line = lines[linenum].decode("utf-8")
    newline = processline(line, numstr, spec)
    lines[linenum] = newline.encode("utf-8")

    global progress
    progress += 1
    sys.stderr.write("\r%d fixes" % progress)
    sys.stderr.flush()


def processline(line, numstr, spec):
    """Replace numstr with spec in line, with some care about escaping"""
    # Do not replace '1' in "hg commit -m 'public 1'".
    # Do not replace '1' in "hg clone repo1 repo2".
    # Do not replace '1' in "hg debugbuilddag '..<2.*1/2:m<2+3:c<m+3:a<2.:b<m+2:d<2.:e<m+1:f'"
    # Do not rewrite non-hg commands, like "initrepo repo1".
    if (
        any(s in line for s in ["hg commit", "hg clone", "debugbuilddag"])
        or "hg" not in line
    ):
        return line

    alnumstr = "abcdefghijklmnopqrstuvwxyz0123456789()"
    if numstr.startswith("-"):
        # A negative rev number.
        alnumstr += "-"
    alnumstr = set(alnumstr)

    newline = ""
    buf = ""
    singlequoted = False
    doublequoted = False

    for ch in line:
        if ch in alnumstr:
            buf += ch
        else:
            # A separator. Append 'buf'.
            # Do not rewrite a commit hash, even if it has numbers in it.
            # Do not replace '1' in 'HGPLAIN=1'.
            # Do not replace '1' in '-R repo1' or '--cwd repo1'.
            # Do not rewrite bookmark-like name (ex. foo@1).
            # Do not rewrite redirections like `2>&1`.
            # Do not rewrite recursively (ex. 2 -> desc(a2) -> desc(adesc(2))).
            if (
                not commithashre.match(buf)
                and not any(
                    newline.endswith(c)
                    for c in ("=", "&", ">", "<", "@", "-R ", "--cwd ")
                )
                and ch not in {">", "<"}
                and spec not in buf
                and "desc(" not in buf
            ):
                if singlequoted or doublequoted or "(" not in spec:
                    buf = buf.replace(numstr, spec)
                else:
                    buf = buf.replace(numstr, quote(spec))
            newline += buf
            buf = ""

            if ch == "'" and not newline.endswith("\\"):
                singlequoted = not singlequoted
            elif ch == '"' and not newline.endswith("\\"):
                doublequoted = not doublequoted

            newline += ch

    return newline


with open(".testrevnum", "rb") as f:
    exec(f.read().decode("utf-8"))

for path, lines in contents.items():
    sys.stderr.write("\rwriting %s\n" % path)
    sys.stderr.flush()
    with open(path, "wb") as f:
        f.write(b"".join(lines))

with open(".testrevnum", "wb") as f:
    f.write(b"")
