# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


# Test whereami

sh % "hg init repo1"
sh % "cd repo1"
sh % "cat" << r"""
[extensions]
whereami=
""" > ".hg/hgrc"

sh % "hg whereami" == "0000000000000000000000000000000000000000"

sh % "echo a" > "a"
sh % "hg add a"
sh % "hg commit -m a"

sh % "hg whereami" == "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b"

sh % "echo b" > "b"
sh % "hg add b"
sh % "hg commit -m b"

sh % "hg up '.^'" == "0 files updated, 0 files merged, 1 files removed, 0 files unresolved"

sh % "echo c" > "c"
sh % "hg add c"
sh % "hg commit -m c"

sh % "hg merge 1" == r"""
    1 files updated, 0 files merged, 0 files removed, 0 files unresolved
    (branch merge, don't forget to commit)"""

sh % "hg whereami" == r"""
    d36c0562f908c692f5204d606d4ff3537d41f1bf
    d2ae7f538514cd87c17547b0de4cea71fe1af9fb"""
