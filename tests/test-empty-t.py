# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Create an empty repo:

sh % "hg init a"
sh % "cd a"

# Try some commands:

sh % "hg log"
sh % "hg histgrep wah" == "[1]"
sh % "hg manifest"
sh % "hg verify" == r"""
    checking changesets
    checking manifests
    crosschecking files in changesets and manifests
    checking files
    0 files, 0 changesets, 0 total revisions"""

# Check the basic files created:

sh % "ls .hg" == r"""
    00changelog.i
    blackbox
    requires
    store
    treestate"""

# Should be empty:
# It's not really empty, though.

sh % "ls .hg/store" == r"""
    allheads
    metalog
    requires"""

# Poke at a clone:

sh % "cd .."
sh % "hg clone a b" == r"""
    updating to branch default
    0 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd b"
sh % "hg verify" == r"""
    checking changesets
    checking manifests
    crosschecking files in changesets and manifests
    checking files
    0 files, 0 changesets, 0 total revisions"""
sh % "ls .hg" == r"""
    00changelog.i
    blackbox
    hgrc
    requires
    store
    treestate"""

# Should be empty:
# It's not really empty, though.

sh % "ls .hg/store" == r"""
    allheads
    metalog"""

sh % "cd .."
