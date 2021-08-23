# coding=utf-8

# Copyright (c) Facebook, Inc. and its affiliates.
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

sh % "hg debugmetalogroots -v" == r"""
    14 1970-01-01 00:00:00 +0000 b254bf03af23e9a47542740bec7c63f1fbe8719f metaedit -mE1 Parent: c5e54efbacd5ed2a8a09843561d96c085ba...
    13 1970-01-01 00:00:00 +0000 c5e54efbacd5ed2a8a09843561d96c085baa52a4 debugdrawdag Parent: c4d41f08609e0a388e2b0a7366e69fa260fe...
    12 1970-01-01 00:00:00 +0000 c4d41f08609e0a388e2b0a7366e69fa260fe8d24 debugdrawdag Parent: b020f3d34ee956d27783379c2b432ccc0aba...
    11 1970-01-01 00:00:00 +0000 b020f3d34ee956d27783379c2b432ccc0aba1449 debugdrawdag Parent: 550da1dcb69ebdea5d8139239411aea33bc2...
    10 1970-01-01 00:00:00 +0000 550da1dcb69ebdea5d8139239411aea33bc262d6 debugdrawdag Parent: a5a4232c5e0b07b4ccf9a73923a5004f5afe...
     9 1970-01-01 00:00:00 +0000 a5a4232c5e0b07b4ccf9a73923a5004f5afeb013 bookmark foo Parent: 4a6e8b2c747549036f471272037e73ef4afe...
     8 1970-01-01 00:00:00 +0000 4a6e8b2c747549036f471272037e73ef4afe473b bookmark foo Parent: e8ed92ee0a467995b6e22c66e36be62dd150...
     7 1970-01-01 00:00:00 +0000 e8ed92ee0a467995b6e22c66e36be62dd150f14f debugdrawdag Parent: 6c2fde8f3fe01db6645f9e16079ec29e57ed...
     6 1970-01-01 00:00:00 +0000 6c2fde8f3fe01db6645f9e16079ec29e57ed4931 debugdrawdag Parent: 465e236e68eafefdfa7614dd6712191507d3...
     5 1970-01-01 00:00:00 +0000 465e236e68eafefdfa7614dd6712191507d34dda debugdrawdag Parent: 831553f8d66e72100b4e740cdcda1856f642...
     4 1970-01-01 00:00:00 +0000 831553f8d66e72100b4e740cdcda1856f642029e debugdrawdag Parent: 7ea650cbc16779a455cb76f3e49ba2f65b3c...
     3 1970-01-01 00:00:00 +0000 7ea650cbc16779a455cb76f3e49ba2f65b3c68ee debugdrawdag Parent: 58db3523f6fdb6c86b76a69c31d17b8f5315...
     2 1970-01-01 00:00:00 +0000 58db3523f6fdb6c86b76a69c31d17b8f53153794 debugdrawdag Parent: 433fb6a14b4e7044062a8886ddcb13ffa34a...
     1 1970-01-01 00:00:00 +0000 433fb6a14b4e7044062a8886ddcb13ffa34a78c1 migrate from vfs
     0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=4a6e8b2c747549036f471272037e73ef4afe473b hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  C C
    │
    o  B B
    │
    o  A A foo

    hint[metalog-root-override]: MetaLog root was overridden to 4a6e8b2c747549036f471272037e73ef4afe473b by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""

sh % "hg debugcompactmetalog" == ""

sh % "hg debugmetalogroots -v" == r"""
    1 1970-01-01 00:00:00 +0000 b254bf03af23e9a47542740bec7c63f1fbe8719f metaedit -mE1 Parent: c5e54efbacd5ed2a8a09843561d96c085ba...
    0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""
