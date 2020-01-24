# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "setconfig treemanifest.flatcompat=0"

sh % "enable amend rebase"
sh % "setconfig 'rebase.singletransaction=True'"
sh % "setconfig 'experimental.copytrace=off'"
sh % "setconfig 'rebase.experimental.inmemory=1'"
sh % "setconfig 'rebase.experimental.inmemory.nomergedriver=False'"
sh % "setconfig 'rebase.experimental.inmemorywarning=rebasing in-memory!'"

sh.cd(testtmp.TESTTMP)

# Create a commit with a move + content change:
sh % "newrepo"
sh % "echo 'original content'" > "file"
sh % "hg add -q"
sh % "hg commit -q -m base"
sh % "echo 'new content'" > "file"
sh % "hg mv file file_new"
sh % "hg commit -m a"
sh % "hg book -r . a"

# Recreate the same commit:
sh % "hg up -q '.~1'"
sh % "echo 'new content'" > "file"
sh % "hg mv file file_new"
sh % "hg commit -m b"
sh % "hg book -r . b"

reponame = "../without-imm"
sh.cp("-R", ".", reponame)

# Rebase one version onto the other, confirm it gets rebased out:
sh % "hg rebase -r b -d a" == r"""
    rebasing in-memory!
    rebasing 811ec875201f "b" (b)
    note: rebase of 2:811ec875201f created no changes to commit"""

# Without IMM, this behavior is semi-broken: the commit is not rebased out and the
# created commit is empty. (D8676355)
sh.cd(reponame)
sh % "setconfig 'rebase.experimental.inmemory=0'"
sh % "hg rebase -r b -d a" == r"""
    rebasing 811ec875201f "b" (b)
    warning: can't find ancestor for 'file_new' copied from 'file'!"""

sh % "hg export tip" == r"""
    # HG changeset patch
    # User test
    # Date 0 0
    #      Thu Jan 01 00:00:00 1970 +0000
    # Node ID 0fe513c05d7fe2819c3ceccb072e74940604af36
    # Parent  24483d5afe6cb1a13b3642b4d8622e91f4d1bec1
    b"""
