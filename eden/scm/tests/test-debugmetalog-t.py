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
    "mutation.date=0 0",
    # Do not track config changes to stabilize the test a bit.
    "metalog.track-config=0",
    "hint.ack-graph-renderer=true",
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

sh % "hg debugmetalogroots" == r"""
    14 1970-01-01 00:00:00 +0000 8eaa516e2e077236f414a272b250e9d527529b45 metaedit -mE1
    13 1970-01-01 00:00:00 +0000 d9c406034b810eeef8f7b3eb2c6c1e92ef028d8a debugdrawdag
    12 1970-01-01 00:00:00 +0000 2a95f482a3eb521b34affdd0a5765de4f9f72359 debugdrawdag
    11 1970-01-01 00:00:00 +0000 ef519daf316aa6c1466032ddcbb501382e8f021c debugdrawdag
    10 1970-01-01 00:00:00 +0000 ef3b2a4c7cc16141ae522c3921119e8158b5adce debugdrawdag
     9 1970-01-01 00:00:00 +0000 e0e927b9d3cea88f8672be61119c38a6ef89ff98 bookmark foo
     8 1970-01-01 00:00:00 +0000 b493e330e05151c2415defbd36bc66eb2f4ea8d9 bookmark foo
     7 1970-01-01 00:00:00 +0000 879d633e11615f2d0430582b715586d065262109 debugdrawdag
     6 1970-01-01 00:00:00 +0000 1af3b43fced1c71f0b44b8215d892f6daae95bf4 debugdrawdag
     5 1970-01-01 00:00:00 +0000 e9dab1312189f80b652c9f48d4389783335170ad debugdrawdag
     4 1970-01-01 00:00:00 +0000 91f0faa0107e6b1642959fd9b5817631f74e9e32 debugdrawdag
     3 1970-01-01 00:00:00 +0000 909095f108333e6e91fd408eb36cddc43e1a151f debugdrawdag
     2 1970-01-01 00:00:00 +0000 228087ca5bdee658224713cccd1c46d0ac353fc7 debugdrawdag
     1 1970-01-01 00:00:00 +0000 8d1691b6882cdeb697a631e964012e936d7c693a migrate from vfs
     0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=b493e330e05151c2415defbd36bc66eb2f4ea8d9 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  C C
    |
    o  B B
    |
    o  A A foo

    hint[metalog-root-override]: MetaLog root was overridden to b493e330e05151c2415defbd36bc66eb2f4ea8d9 by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""
