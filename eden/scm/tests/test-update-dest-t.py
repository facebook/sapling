# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Test update.requiredest
sh % 'cd "$TESTTMP"'
sh % "cat" << r"""
[commands]
update.requiredest = True
""" >> "$HGRCPATH"
sh % "hg init repo"
sh % "cd repo"
sh % "echo a" >> "a"
sh % "hg commit -qAm aa"
sh % "hg up" == r"""
    abort: you must specify a destination
    (for example: hg update ".::")
    [255]"""
sh % "hg up ." == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "'HGPLAIN=1' hg up" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg --config 'commands.update.requiredest=False' up" == "0 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "cd .."

# Check update.requiredest interaction with pull --update
sh % "hg clone repo clone" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd repo"
sh % "echo a" >> "a"
sh % "hg commit -qAm aa"
sh % "cd ../clone"
sh % "hg pull --update" == r"""
    abort: update destination required by configuration
    (use hg pull followed by hg update DEST)
    [255]"""

sh % "cd .."

# update.requiredest should silent the "hg update" text after pull
sh % "hg init repo1"
sh % "cd repo1"
sh % "hg pull ../repo" == r"""
    pulling from ../repo
    requesting all changes
    adding changesets
    adding manifests
    adding file changes
    added 2 changesets with 2 changes to 1 files
    new changesets 8f0162e483d0:048c2cb95949"""
