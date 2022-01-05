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
    6 1970-01-01 00:00:00 +0000 b9ff20d189500cf34c798af0ef8cb196e4a7d0e3 metaedit -mE1 Parent: 26f65c1b1661193443c11c5e5a4c3223293...
    5 1970-01-01 00:00:00 +0000 26f65c1b1661193443c11c5e5a4c32232936b306 debugdrawdag Parent: 4e75df2d0d69608b519da6fa6dc2c62de421...
    4 1970-01-01 00:00:00 +0000 4e75df2d0d69608b519da6fa6dc2c62de421f586 bookmark foo Parent: 777aa96d0e876fa97d03864534426986580b...
    3 1970-01-01 00:00:00 +0000 777aa96d0e876fa97d03864534426986580b5882 bookmark foo Parent: 83b3ee2a447f1e4c4e28df17c836e78bdeff...
    2 1970-01-01 00:00:00 +0000 83b3ee2a447f1e4c4e28df17c836e78bdefff009 debugdrawdag Parent: 433fb6a14b4e7044062a8886ddcb13ffa34a...
    1 1970-01-01 00:00:00 +0000 433fb6a14b4e7044062a8886ddcb13ffa34a78c1 migrate from vfs
    0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=26f65c1b1661193443c11c5e5a4c32232936b306 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  E E
    │
    o  D D
    │
    │ o  C C foo
    ├─╯
    o  B B
    │
    o  A A

    hint[metalog-root-override]: MetaLog root was overridden to 26f65c1b1661193443c11c5e5a4c32232936b306 by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""

sh % "hg debugcompactmetalog" == ""

sh % "hg debugmetalogroots -v" == r"""
    1 1970-01-01 00:00:00 +0000 b9ff20d189500cf34c798af0ef8cb196e4a7d0e3 metaedit -mE1 Parent: 26f65c1b1661193443c11c5e5a4c3223293...
    0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""
