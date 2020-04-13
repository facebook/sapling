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
    14 1970-01-01 00:00:00 +0000 167b807aecfe61c2ba7c1a6a3c439014b8e3db2e metaedit -mE1
    13 1970-01-01 00:00:00 +0000 6ef2a93528c9d568c5a605cc1bde7fb1506e6e2a debugdrawdag
    12 1970-01-01 00:00:00 +0000 fcc38e5bd982b80c710a2d5a9882356f68fdb3c1 debugdrawdag
    11 1970-01-01 00:00:00 +0000 f876231478a8f881cae704d9cce99883d39b7952 debugdrawdag
    10 1970-01-01 00:00:00 +0000 48cf11751e653d681bb08568fbe1b3f4369d71a1 debugdrawdag
     9 1970-01-01 00:00:00 +0000 8b153fa88c1ec6dba7153e71c11cd8f85ca659cb bookmark foo
     8 1970-01-01 00:00:00 +0000 3be0a703faa858cb462b268d33db8a19ff20400e bookmark foo
     7 1970-01-01 00:00:00 +0000 dcc2c00fe5550d688fc176f50ebec67f80064825 debugdrawdag
     6 1970-01-01 00:00:00 +0000 ac768096fcdf054eb19806ab5ad24676fbc63bf8 debugdrawdag
     5 1970-01-01 00:00:00 +0000 33c9042f16898f581307bace0e664cc982b93ceb debugdrawdag
     4 1970-01-01 00:00:00 +0000 cf07fd7a4a303ed5c2e59a96941f26107921683e debugdrawdag
     3 1970-01-01 00:00:00 +0000 69f5aee46a0c432dd128626f018960c673121f62 debugdrawdag
     2 1970-01-01 00:00:00 +0000 4a37b9ad6ab30c699a0271bb1a9e6fc67bd3acef debugdrawdag
     1 1970-01-01 00:00:00 +0000 b996330fd2940eb3710fcce9f286564498f7e1d0 migrate from vfs
     0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d"""

sh % "hg up -q null"

sh % "HGFORCEMETALOGROOT=b996330fd2940eb3710fcce9f286564498f7e1d0 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    hint[metalog-root-override]: MetaLog root was overridden to b996330fd2940eb3710fcce9f286564498f7e1d0 by an environment variable. This should only be used for debugging.
    hint[hint-ack]: use 'hg hint --ack metalog-root-override' to silence these hints"""

sh.setconfig("hint.ack=*")
sh % "HGFORCEMETALOGROOT=dcc2c00fe5550d688fc176f50ebec67f80064825 hg log -G -r 'all()' -T '{desc} {bookmarks}'" == r"""
    o  C C
    |
    o  B B
    |
    o  A A"""
