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

sh % "hg debugmetalogroots -v" == r"""
    14 1970-01-01 00:00:00 +0000 e101d079054ded8fb61926aa99c6abb5162925f5 metaedit -mE1 Transaction: metaedit
    13 1970-01-01 00:00:00 +0000 ff82865a3d1b68db09dd5bdf19a14b0d9af724b0 debugdrawdag Transaction: bookmark
    12 1970-01-01 00:00:00 +0000 ce551e7adf176b648a9fca3ff1dfd0d168e9b9ab debugdrawdag Transaction: commit
    11 1970-01-01 00:00:00 +0000 6526ac5762293b2f27d3d367492ec2f7941b8250 debugdrawdag Transaction: bookmark
    10 1970-01-01 00:00:00 +0000 590432f4f161f2ab0b5cf78fd64c9aee54572c51 debugdrawdag Transaction: commit
     9 1970-01-01 00:00:00 +0000 64ba23c13141797476fbf8dda015c9a733b25d1f bookmark foo Transaction: bookmark
     8 1970-01-01 00:00:00 +0000 280022070a10d2a1a752f6e0951c7649fa3aeed0 bookmark foo Transaction: bookmark
     7 1970-01-01 00:00:00 +0000 2b568bbe60079854ed4204d6c23632ec148ba374 debugdrawdag Transaction: bookmark
     6 1970-01-01 00:00:00 +0000 6002154bee3bcd116f66ce7e8a39a17863d30bf4 debugdrawdag Transaction: commit
     5 1970-01-01 00:00:00 +0000 8cbb80514b72ecf0e96d4aac3bd6138179acaa8e debugdrawdag Transaction: bookmark
     4 1970-01-01 00:00:00 +0000 c7b19359bc92edd4ff376128f551fa47d2167a22 debugdrawdag Transaction: commit
     3 1970-01-01 00:00:00 +0000 c94b525421a275f3af67c8780587af81b2eac8ae debugdrawdag Transaction: bookmark
     2 1970-01-01 00:00:00 +0000 d4275ab8efc006b015186bd434e7935dd7d653f7 debugdrawdag Transaction: commit
     1 1970-01-01 00:00:00 +0000 433fb6a14b4e7044062a8886ddcb13ffa34a78c1 migrate from vfs
     0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=280022070a10d2a1a752f6e0951c7649fa3aeed0 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  C C
    │
    o  B B
    │
    o  A A foo

    hint[metalog-root-override]: MetaLog root was overridden to 280022070a10d2a1a752f6e0951c7649fa3aeed0 by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""
