# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "rm -rf a"
sh % "hg init a"
sh % "cd a"
sh % "echo a" > "a"
sh % "hg ci -Am0" == "adding a"
sh % "hg tag t1"
sh % "hg tag --remove t1"

sh % "hg co 1" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"
sh % "hg tag -f -r0 t1"
sh % "hg tags" == r"""
    tip                                3:a49829c4fc11
    t1                                 0:f7b1eb17ad24"""

sh % "cd .."
