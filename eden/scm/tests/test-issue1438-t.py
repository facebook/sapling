# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


feature.require(["symlink"])

# https://bz.mercurial-scm.org/1438

sh % "hg init"

sh % "ln -s foo link"
sh % "hg add link"
sh % "hg ci -mbad link"
sh % "hg rm link"
sh % "hg ci -mok"
sh % "hg diff -g -r '0:1'" > "bad.patch"

sh % "hg up 0" == "1 files updated, 0 files merged, 0 files removed, 0 files unresolved"

sh % "hg import --no-commit bad.patch" == "applying bad.patch"

sh % "hg status" == r"""
    R link
    ? bad.patch"""
