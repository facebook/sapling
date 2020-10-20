# coding=utf-8

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
    │  1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)
    │  1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    │
    │ o  26805aba1e60 C
    ├─╯  1970-01-01 00:00:00 +0000: foo (added by bookmark foo)
    │    1970-01-01 00:00:00 +0000: C (added by debugdrawdag)
    │    1970-01-01 00:00:00 +0000: . (added by debugdrawdag)
    │
    o  112478962961 B
    │  1970-01-01 00:00:00 +0000: B (added by debugdrawdag)
    │  1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)
    │
    o  426bada5c675 A
       1970-01-01 00:00:00 +0000: foo (removed by bookmark foo)
       1970-01-01 00:00:00 +0000: foo (added by bookmark foo)
       1970-01-01 00:00:00 +0000: . (removed by debugdrawdag)"""

sh % "hg debugmetalogroots" == r"""
    14 1970-01-01 00:00:00 +0000 d556f0b5580cfe679acdf066664c8509f7099e79 metaedit -mE1
    13 1970-01-01 00:00:00 +0000 c54a575924de1145400aae88a877b1653d1c4b95 debugdrawdag
    12 1970-01-01 00:00:00 +0000 edc497739c37970bc19d8fd0752a3b5751207779 debugdrawdag
    11 1970-01-01 00:00:00 +0000 9b58f005a1d505ac7a7fa04ef9fa86afb1a15755 debugdrawdag
    10 1970-01-01 00:00:00 +0000 860ed24ef5c5d2b37c3a040b0dddf71846fddb1b debugdrawdag
     9 1970-01-01 00:00:00 +0000 18d59faa603480c16c69d486fcb60a18ca0f1ea1 bookmark foo
     8 1970-01-01 00:00:00 +0000 26ee461e6a3c56f87cb28103297708c8b436dee7 bookmark foo
     7 1970-01-01 00:00:00 +0000 303519cbdad17997a9a54cd782bf8f8666f3e243 debugdrawdag
     6 1970-01-01 00:00:00 +0000 c6e11680632adbf09c5d20c7666af403fd7679b8 debugdrawdag
     5 1970-01-01 00:00:00 +0000 5cfa7fb3a42a3f9ed6812e19eefe13aa91f44766 debugdrawdag
     4 1970-01-01 00:00:00 +0000 58cdf865a5fa02ef538b487a8be1d304e623e266 debugdrawdag
     3 1970-01-01 00:00:00 +0000 82e9af13815bdc336c14a4385afe60b061ee3569 debugdrawdag
     2 1970-01-01 00:00:00 +0000 5d6757bf3e077c83e4e82be112c10b20baa9a0b3 debugdrawdag
     1 1970-01-01 00:00:00 +0000 433fb6a14b4e7044062a8886ddcb13ffa34a78c1 migrate from vfs
     0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=26ee461e6a3c56f87cb28103297708c8b436dee7 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  C C
    │
    o  B B
    │
    o  A A foo

    hint[metalog-root-override]: MetaLog root was overridden to 26ee461e6a3c56f87cb28103297708c8b436dee7 by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""
