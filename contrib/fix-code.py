#!/usr/bin/env python

"""Automatically fix code

Check and fix the following things:
- Cargo.toml
    - Change version = "*" to actual version (requires Cargo.lock)
"""
from __future__ import absolute_import

import glob
import os
import re
import sys
from distutils.version import LooseVersion


HAVE_FB = os.path.exists(
    os.path.join(os.path.dirname(os.path.abspath(__file__)), "../fb")
)


def fixcargotoml(path):
    """Fix Cargo.toml:

    - Change 'version = "*"' to an actual semver. This makes old code
      buildable in the GitHub export.

    This does not do a "proper" parsing of Cargo.toml. Note that the existing
    toml library is not friendly to do automated editing. For example, the
    file content does not round-trip deserialization + serialization.
    """

    versionre = re.compile(r'^((\w+)\s*=.*)"\*"(.*)', re.DOTALL)
    content = open(path).read()
    newcontent = ""

    # Replace version = "*" to the version specified in Cargo.lock.
    # This is gated to the FB version so external build won't be chruned.
    if HAVE_FB:
        for line in content.splitlines(True):
            m = versionre.match(line)
            if m:
                left, crate, right = m.groups()
                line = '%s"%s"%s' % (left, crateversion(crate), right)
            newcontent += line

    if content != newcontent:
        write(path, newcontent)


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
            "newdoc",
            "pywatchman",
            # Part of "tests" are hg-git code.
            "tests",
            "third-party",
            "thirdparty",
        ]
    )


def fixpaths(paths):
    for path in paths:
        if ispathskipped(path):
            continue
        if os.path.basename(path) == "Cargo.toml":
            fixcargotoml(path)


_crateversions = {}  # {crate: version} pinned by Cargo.lock


def crateversion(crate):
    """Read Cargo.lock to find out the version to use. Return the version.
    For example, crateversion("libc") might return a string '2.1'.
    """
    if not _crateversions:
        # Insert a placeholder to avoid loading files again
        _crateversions["_"] = "*"

        # Load Cargo.lock from predefined locations
        root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
        paths = []
        for pattern in [
            "lib/Cargo.lock",
            "edenscm/hgext/extlib/*/Cargo.lock",
            "edenscm/mercurial/rust/*/Cargo.lock",
            "exec/*/Cargo.lock",
        ]:
            paths += list(glob.glob(os.path.join(root, pattern)))

        for path in paths:
            currentcrate = None
            for line in open(path).read().splitlines():
                name, value = (line.split(" = ", 1) + [None])[:2]
                if value is not None:
                    if name == "name":
                        currentcrate = value.replace('"', "")
                    elif name == "version":
                        value = value.lstrip('"').rstrip('"')
                        # Pick the latest version
                        oldversion = _crateversions.get(currentcrate, None)
                        if not oldversion or LooseVersion(oldversion) < LooseVersion(
                            value
                        ):
                            _crateversions[currentcrate] = value
    return _crateversions.get(crate, "*")


if __name__ == "__main__":
    if sys.argv[1] == "--dry-run":

        def write(path, content):
            print("Need fix: %s" % path)

        paths = sys.argv[2:]
    else:

        def write(path, content):
            print("Fixing: %s" % path)
            with open(path, "w") as f:
                f.write(content)

        paths = sys.argv[1:]
    fixpaths(paths)
