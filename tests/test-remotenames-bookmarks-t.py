# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
remotenames=
""" >> "$HGRCPATH"

# Setup repo

sh % "hg init repo"
sh % "cd repo"
sh % "echo foo" > "a.txt"
sh % "hg add a.txt"
sh % "hg commit -m a"

# Testing bookmark options without args
sh % "hg bookmark a"
sh % "hg bookmark b"
sh % "hg bookmark -v" == r"""
       a                         0:2dcb9139ea49
     * b                         0:2dcb9139ea49"""
sh % "hg bookmark --track a"
sh % "hg bookmark -v" == r"""
       a                         0:2dcb9139ea49
     * b                         0:2dcb9139ea49            [a]"""
sh % "hg bookmark --untrack"
sh % "hg bookmark -v" == r"""
       a                         0:2dcb9139ea49
     * b                         0:2dcb9139ea49"""
