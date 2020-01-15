# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"
# Set up without remotenames
sh % "cat" << r"""
[extensions]
rebase=
tweakdefaults=
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "echo a" > "repo/a"
sh % "hg -R repo commit -qAm a"
sh % "hg clone -q repo clone"
sh % "cd clone"

# Pull --rebase with no local changes
sh % "echo b" > "../repo/b"
sh % "hg -R ../repo commit -qAm b"
sh % "hg pull --rebase -d default" == r"""
    pulling from $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    nothing to rebase - fast-forwarded to default"""
sh % "hg log -G -T '{rev} {desc}'" == r"""
    @  1 b
    |
    o  0 a"""
# Make a local commit and check pull --rebase still works.
sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "echo c" > "../repo/c"
sh % "hg -R ../repo commit -qAm c"
sh % "hg pull --rebase -d default" == r"""
    pulling from $TESTTMP/repo
    searching for changes
    adding changesets
    adding manifests
    adding file changes
    added 1 changesets with 1 changes to 1 files
    rebasing 86d71924e1d0 "x"
    saved backup bundle to $TESTTMP/clone/.hg/strip-backup/86d71924e1d0-48875604-rebase.hg"""
sh % "hg log -G -T '{rev} {desc}'" == r"""
    @  3 x
    |
    o  2 c
    |
    o  1 b
    |
    o  0 a"""
