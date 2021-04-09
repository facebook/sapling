# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "hg init test"
sh % "cd test"
sh % "echo a" > "changed"
sh % "echo a" > "removed"
sh % "echo a" > "source"
sh % "hg ci -Am addfiles" == r"""
    adding changed
    adding removed
    adding source"""
sh % "echo a" >> "changed"
sh % "echo a" > "added"
sh % "hg add added"
sh % "hg rm removed"
sh % "hg cp source copied"
sh % "hg diff --git" > "../unknown.diff"

# Test adding on top of an unknown file

sh % "hg up -qC 0"
sh % "hg purge"
sh % "echo a" > "added"
sh % "hg import --no-commit ../unknown.diff" == r"""
    applying ../unknown.diff
    file added already exists
    1 out of 1 hunks FAILED -- saving rejects to file added.rej
    abort: patch failed to apply
    [255]"""

# Test modifying an unknown file

sh % "hg revert -aq"
sh % "hg purge"
sh % "hg rm changed"
sh % "hg ci -m removechanged"
sh % "echo a" > "changed"
sh % "hg import --no-commit ../unknown.diff" == r"""
    applying ../unknown.diff
    abort: cannot patch changed: file is not tracked
    [255]"""

# Test removing an unknown file

sh % "hg up -qC 0"
sh % "hg purge"
sh % "hg rm removed"
sh % "hg ci -m removeremoved"
sh % "echo a" > "removed"
sh % "hg import --no-commit ../unknown.diff" == r"""
    applying ../unknown.diff
    abort: cannot patch removed: file is not tracked
    [255]"""

# Test copying onto an unknown file

sh % "hg up -qC 0"
sh % "hg purge"
sh % "echo a" > "copied"
sh % "hg import --no-commit ../unknown.diff" == r"""
    applying ../unknown.diff
    abort: cannot create copied: destination already exists
    [255]"""

sh % "cd .."
