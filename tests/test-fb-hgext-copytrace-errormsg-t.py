# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# TODO: Make this test compatibile with obsstore enabled.
sh % "setconfig 'experimental.evolution='"

sh % "cat" << r"""
[extensions]
rebase=
copytrace=
[experimental]
copytrace=off
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"
sh % "echo 1" > "1"
sh % "hg add 1"
sh % "hg ci -m 1"
sh % "echo 2" > "1"
sh % "hg ci -m 2"
sh % "hg up 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg mv 1 2"
sh % "hg ci -m dest"
sh % "hg rebase -s 1 -d ." == r"""
    rebasing 1:812796267395 "2"
    other [source] changed 1 which local [dest] deleted
    hint: if this message is due to a moved file, you can ask mercurial to attempt to automatically resolve this change by re-running with the --tracecopies flag, but this will significantly slow down the operation, so you will need to be patient.
    Source control team is working on fixing this problem.
    use (c)hanged version, leave (d)eleted, or leave (u)nresolved? u
    unresolved conflicts (see hg resolve, then hg rebase --continue)
    [1]"""
sh % "hg rebase --abort" == "rebase aborted"
sh % "hg rebase -s 1 -d . --tracecopies" == r"""
    rebasing 1:812796267395 "2"
    merging 2 and 1 to 2
    saved backup bundle to $TESTTMP/repo/.hg/strip-backup/812796267395-81e11405-rebase.hg (glob)"""
