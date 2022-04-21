# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh.setconfig(
    "visibility.enabled=true",
    "experimental.narrow-heads=1",
    "remotenames.selectivepull=1",
    "mutation.date=0 0",
    # Do not track config changes to stabilize the test a bit.
    "metalog.track-config=0",
)
sh.newrepo()
sh.enable("remotenames", "amend")

(
    sh % "hg debugdrawdag"
    << r"""
C
|
B
|
A
"""
)

sh.hg("update", "desc(A)")
sh.hg("bookmark", "foo")
sh.hg("update", "desc(C)")
sh.hg("bookmark", "foo")

(
    sh % "hg debugdrawdag"
    << r"""
E
|
D
|
desc(B)
"""
)

sh.hg("update", "desc(E)")
sh.hg("metaedit", "-mE1")

sh % "hg debugmetalog" == r"""
    @  25b25cf4a935 E1
    │  1970-01-01 00:00:00 +0000: E (added by metaedit -mE1)
    │  1970-01-01 00:00:00 +0000: . (added by metaedit -mE1)
    │
    │ x  a6c8ab8ac0c6 E
    ├─╯  1970-01-01 00:00:00 +0000: E (removed by metaedit -mE1)
    │    1970-01-01 00:00:00 +0000: E (added by debugdrawdag)
    │    1970-01-01 00:00:00 +0000: . (removed by metaedit -mE1)
    │    1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    │
    o  be0ef73c17ad D
    │  1970-01-01 00:00:00 +0000: D (added by debugdrawdag)
    │
    o  112478962961 B
    │
    o  426bada5c675 A
       1970-01-01 00:00:00 +0000: foo (removed by bookmark foo)"""

sh % "hg debugmetalogroots -v" == r"""
    6 1970-01-01 00:00:00 +0000 3b7405a1d14a8309e9a202d78c2f0b28c2e248cc metaedit -mE1 Parent: 91a2a0dd2d7239bb660b51c333b5017c7f6...
    5 1970-01-01 00:00:00 +0000 91a2a0dd2d7239bb660b51c333b5017c7f60147d debugdrawdag Parent: 1463d4272581f658497076020478f54ef3bf...
    4 1970-01-01 00:00:00 +0000 1463d4272581f658497076020478f54ef3bfb0f7 bookmark foo Parent: 52ac39c1f422dc12cd041a6c8c35527e179e...
    3 1970-01-01 00:00:00 +0000 52ac39c1f422dc12cd041a6c8c35527e179ef5c0 bookmark foo Parent: d75fe20c6a8a12d95c49c622bfa346272833...
    2 1970-01-01 00:00:00 +0000 d75fe20c6a8a12d95c49c622bfa346272833acea debugdrawdag Parent: 22f7ca48c27ae55149b47e140c3f5b9a2bac...
    1 1970-01-01 00:00:00 +0000 22f7ca48c27ae55149b47e140c3f5b9a2bac9e95 init tracked
    0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=91a2a0dd2d7239bb660b51c333b5017c7f60147d hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  E E
    │
    o  D D
    │
    │ o  C C foo
    ├─╯
    o  B B
    │
    o  A A"""

sh % "hg debugcompactmetalog" == ""

sh % "hg debugmetalogroots -v" == r"""
    1 1970-01-01 00:00:00 +0000 3b7405a1d14a8309e9a202d78c2f0b28c2e248cc metaedit -mE1 Parent: 91a2a0dd2d7239bb660b51c333b5017c7f6...
    0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""
