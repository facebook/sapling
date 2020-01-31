# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig 'extensions.treemanifest=!'"
# Set up extension and repos

sh % "echo '[extensions]'" >> "$HGRCPATH"
sh % "echo 'remotenames='" >> "$HGRCPATH"
sh % "echo 'color='" >> "$HGRCPATH"
sh % "echo '[color]'" >> "$HGRCPATH"
sh % "echo 'log.remotebookmark = yellow'" >> "$HGRCPATH"
sh % "echo 'log.remotebranch = red'" >> "$HGRCPATH"
sh % "echo 'log.hoistedname = blue'" >> "$HGRCPATH"
sh % "hg init repo1"
sh % "cd repo1"
sh % "echo a" > "a"
sh % "hg add a"
sh % "hg commit -qm a"
sh % "hg boo bm2"
sh % "cd .."
sh % "hg clone repo1 repo2" == r"""
    updating to branch default
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved"""
sh % "cd repo2"
sh % "hg bookmark local"

# Test colors

sh % "hg log '--color=always' -l 1" == r"""
    [0;33mchangeset:   0:cb9a9f314b8b[0m
    bookmark:    local
    [0;33mbookmark:    default/bm2[0m
    [0;34mhoistedname: bm2[0m
    user:        test
    date:        Thu Jan 01 00:00:00 1970 +0000
    summary:     a"""
