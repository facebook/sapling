# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Issue1678: IndexError when pushing

# setting up base repo
sh % "hg init a"
sh % "cd a"
sh % "touch a"
sh % "hg ci -Am a" == "adding a"
sh % "cd .."

# cloning base repo
sh % "hg clone a b" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd b"

# setting up cset to push
sh % "hg up null" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"
sh % "touch a"
# different msg so we get a clog new entry
sh % "hg ci -Am b" == "adding a"

# pushing
sh % "hg push -f ../a" == r"""
    pushing to ../a
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 0 changes to 0 files"""

sh % "cd .."
