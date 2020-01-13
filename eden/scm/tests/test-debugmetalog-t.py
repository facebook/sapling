# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh.setconfig(
    "experimental.metalog=1",
    "visibility.enabled=true",
    "experimental.narrow-heads=1",
    "remotenames.selectivepull=1",
)
sh.newrepo()
sh.enable("remotenames", "amend")

sh % "hg debugdrawdag" << r"""
C
|
B
|
A
"""

sh.hg("update", "desc(A)")
sh.hg("bookmark", "foo")
sh.hg("update", "desc(C)")
sh.hg("bookmark", "foo")

sh % "hg debugdrawdag" << r"""
E
|
D
|
desc(B)
"""

sh.hg("update", "desc(E)")
sh.hg("metaedit", "-mE1")

sh % "hg debugmetalog" == r"""
    @  25b25cf4a935 E1
    |  1970-01-01 00:00:00 +0000: E (added by metaedit -mE1)
    |  1970-01-01 00:00:00 +0000: . (added by metaedit -mE1)
    |
    | o  a6c8ab8ac0c6 E
    |/   1970-01-01 00:00:00 +0000: E (removed by metaedit -mE1)
    |    1970-01-01 00:00:00 +0000: E (added by debugdrawdag)
    |    1970-01-01 00:00:00 +0000: . (removed by metaedit -mE1)
    |    1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    |
    o  be0ef73c17ad D
    |  1970-01-01 00:00:00 +0000: D (added by debugdrawdag)
    |  1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)
    |  1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    |
    | o  26805aba1e60 C
    |/   1970-01-01 00:00:00 +0000: foo (added by bookmark foo)
    |    1970-01-01 00:00:00 +0000: C (added by debugdrawdag)
    |    1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    |
    o  112478962961 B
    |  1970-01-01 00:00:00 +0000: B (added by debugdrawdag)
    |  1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)
    |
    o  426bada5c675 A
       1970-01-01 00:00:00 +0000: foo (removed by bookmark foo)
       1970-01-01 00:00:00 +0000: foo (added by bookmark foo)
       1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)"""
