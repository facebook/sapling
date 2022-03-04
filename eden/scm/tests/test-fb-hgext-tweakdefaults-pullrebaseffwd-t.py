# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "configure modernclient"

sh.enable("remotenames")
# Set up without remotenames
(
    sh % "cat"
    << r"""
[extensions]
rebase=
tweakdefaults=
"""
    >> "$HGRCPATH"
)

sh % "newclientrepo repo"
sh % "cd .."
sh % "echo a" > "repo/a"
sh % "hg -R repo commit -qAm a"
sh % "hg -R repo bookmark master"
sh % "hg -R repo push -q -r . --to book --create"
sh % "newclientrepo clone test:repo_server book"

# Pull --rebase with no local changes
sh % "echo b" > "../repo/b"
sh % "hg -R ../repo commit -qAm b"
sh % "hg -R ../repo push -q -r . --to book"
sh % "hg pull --rebase -d book" == r"""
    pulling from test:repo_server
    searching for changes
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    nothing to rebase - fast-forwarded to book"""
sh % "hg log -G -T '{desc} {desc}'" == r"""
    @  b b
    │
    o  a a"""
# Make a local commit and check pull --rebase still works.
sh % "echo x" > "x"
sh % "hg commit -qAm x"
sh % "echo c" > "../repo/c"
sh % "hg -R ../repo commit -qAm c"
sh % "hg -R ../repo push -q -r . --to book"
sh % "hg pull --rebase -d book" == r'''
    pulling from test:repo_server
    searching for changes
    rebasing 86d71924e1d0 "x"'''
sh % "hg log -G -T '{desc} {desc}'" == r"""
    @  x x
    │
    o  c c
    │
    o  b b
    │
    o  a a"""
