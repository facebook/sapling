# Copyright (c) Facebook, Inc. and its affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# https://bz.mercurial-scm.org/612

sh % "hg init"
sh % "mkdir src"
sh % "echo a" > "src/a.c"
sh % "hg ci -Ama" == "adding src/a.c"

sh % "hg mv src source" == "moving src/a.c to source/a.c"

sh % "hg ci -Ammove"

sh % "hg co -C 0" == "1 files updated, 0 files merged, 1 files removed, 0 files unresolved"

sh % "echo new" > "src/a.c"
sh % "echo compiled" > "src/a.o"
sh % "hg ci -mupdate"

sh % "hg status" == "? src/a.o"

sh % "hg merge" == r"""
    merging src/a.c and source/a.c to source/a.c
    0 files updated, 1 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

sh % "hg status" == r"""
    M source/a.c
    R src/a.c
    ? src/a.o"""
