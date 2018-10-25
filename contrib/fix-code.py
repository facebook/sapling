#!/usr/bin/env python

"""Automatically fix code

Check and fix the following things:
- Rust source code
    - Copyright header
- Python source code
    - Copyright header
"""
from __future__ import absolute_import

import os
import subprocess
import sys


def getauthorandyear(path):
    """Returns the appropriate copyright holder and year based on the commit
    introducing the file.

    e.g. ("Facebook, Inc.", 2018)
    """
    # Those lines look like "2010-02-17 20:30 +0100"
    lines = sorted(
        subprocess.check_output(
            ["hg", "log", "-f", "-T{date|shortdate} {author|email}\n", path]
        ).splitlines()
    )
    date, email = lines[0].split()
    year = date.split("-", 1)[0]
    if email.endswith("@fb.com"):
        return "Facebook, Inc.", year
    else:
        return "Mercurial Contributors", year


def fixcopyrightheader(path):
    content = open(path).read()
    if (
        # Split strings to make it possible for fixing this script itself.
        ("General" " Public") in content
        or ("Copy" "right") in content
        or len(content.strip()) == 0
    ):
        return

    print("Fixing %s" % path)

    if path.endswith(".rs"):
        comment = "//"
    else:
        comment = "#"

    author, year = getauthorandyear(path)

    header = (
        "%(comment)s Copy"
        + "right %(year)s %(author)s\n"
        + "%(comment)s\n"
        + "%(comment)s This software may be used and distributed according to the terms of the\n"
        + "%(comment)s GNU General"
        + " Public License version 2 or any later version.\n\n"
    ) % {"year": year, "comment": comment, "author": author}

    if content.startswith("#!") and not path.endswith(".rs"):
        firstline, rest = content.split("\n", 1)
        header = "%s\n\n%s" % (firstline, header)
        content = rest

    with open(path, "w") as f:
        f.write(header)
        f.write(content)


def ispathskipped(path):
    components = set(path.split(os.path.sep))
    return any(
        name in components
        # Third-party or imported projects have different authors or licenses.
        # Documentation does not contain source code.
        for name in [
            "contrib",
            "doc",
            "hggit",
            "hgsubversion",
            "newdoc",
            "pywatchman",
            # Part of "tests" are hg-git or hgsubversion code.
            "tests",
            "third-party",
            "thirdparty",
        ]
    )


def fixpaths(paths):
    for path in paths:
        if ispathskipped(path):
            continue
        if path.endswith(".rs") or path.endswith(".py") or path.endswith(".pyx"):
            fixcopyrightheader(path)


if __name__ == "__main__":
    fixpaths(sys.argv[1:])
