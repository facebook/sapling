# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

from testutil.dott import feature, sh, testtmp  # noqa: F401


sh % "cat" << r"""
[extensions]
fastannotate=
[fastannotate]
mainbranch=main
""" >> "$HGRCPATH"

sh % "hg init repo"
sh % "cd repo"

# add or rename files on top of the master branch

sh % "echo a1" > "a"
sh % "echo b1" > "b"
sh % "hg commit -qAm 1"
sh % "hg bookmark -i main"
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: 1 new changesets in the main branch
    0 b: b1"""
sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: 1 new changesets in the main branch
    0 a: a1"""
sh % "echo a2" >> "a"
sh % "cat" << r"""
b0
b1
""" > "b"
sh % "hg mv a t"
sh % "hg mv b a"
sh % "hg mv t b"
sh % "hg commit -m 'swap names'"

# existing linelogs are not helpful with such renames in side branches

sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: linelog cannot help in annotating this revision
    1 a: b0
    0 b: b1"""
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: linelog cannot help in annotating this revision
    0 a: a1
    1 b: a2"""

# move main branch forward, rebuild should happen

sh % "hg bookmark -i main -r . -q"
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: cache broken and deleted
    fastannotate: b: 2 new changesets in the main branch
    0 a: a1
    1 b: a2"""
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: using fast path (resolved fctx: True)
    0 a: a1
    1 b: a2"""

# for rev 0, the existing linelog is still useful for a, but not for b

sh % "hg fastannotate --debug -nf a -r 0" == r"""
    fastannotate: a: using fast path (resolved fctx: True)
    0 a: a1"""
sh % "hg fastannotate --debug -nf b -r 0" == r"""
    fastannotate: b: linelog cannot help in annotating this revision
    0 b: b1"""

# a rebuild can also be triggered if "the main branch last time" mismatches

sh % "echo a3" >> "a"
sh % "hg commit -m a3"
sh % "cat" << r"""
b3
b4
""" >> "b"
sh % "hg commit -m b4"
sh % "hg bookmark -i main -q"
sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: cache broken and deleted
    fastannotate: a: 3 new changesets in the main branch
    1 a: b0
    0 b: b1
    2 a: a3"""
sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: using fast path (resolved fctx: True)
    1 a: b0
    0 b: b1
    2 a: a3"""

# linelog can be updated without being helpful

sh % "hg mv a t"
sh % "hg mv b a"
sh % "hg mv t b"
sh % "hg commit -m 'swap names again'"
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: 1 new changesets in the main branch
    1 a: b0
    0 b: b1
    2 a: a3"""
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: linelog cannot help in annotating this revision
    1 a: b0
    0 b: b1
    2 a: a3"""

# move main branch forward again, rebuilds are one-time

sh % "hg bookmark -i main -q"
sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: cache broken and deleted
    fastannotate: a: 4 new changesets in the main branch
    0 a: a1
    1 b: a2
    3 b: b3
    3 b: b4"""
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: cache broken and deleted
    fastannotate: b: 4 new changesets in the main branch
    1 a: b0
    0 b: b1
    2 a: a3"""
sh % "hg fastannotate --debug -nf a" == r"""
    fastannotate: a: using fast path (resolved fctx: True)
    0 a: a1
    1 b: a2
    3 b: b3
    3 b: b4"""
sh % "hg fastannotate --debug -nf b" == r"""
    fastannotate: b: using fast path (resolved fctx: True)
    1 a: b0
    0 b: b1
    2 a: a3"""

# list changeset hashes to improve readability

sh % "hg log -T '{rev}:{node}\\n'" == r"""
    4:980e1ab8c516350172928fba95b49ede3b643dca
    3:14e123fedad9f491f5dde0beca2a767625a0a93a
    2:96495c41e4c12218766f78cdf244e768d7718b0f
    1:35c2b781234c994896aba36bd3245d3104e023df
    0:653e95416ebb5dbcc25bbc7f75568c9e01f7bd2f"""

# annotate a revision not in the linelog. linelog cannot be used, but does not get rebuilt either

sh % "hg fastannotate --debug -nf a -r 96495c41e4c12218766f78cdf244e768d7718b0f" == r"""
    fastannotate: a: linelog cannot help in annotating this revision
    1 a: b0
    0 b: b1
    2 a: a3"""
sh % "hg fastannotate --debug -nf a -r 2" == r"""
    fastannotate: a: linelog cannot help in annotating this revision
    1 a: b0
    0 b: b1
    2 a: a3"""
sh % "hg fastannotate --debug -nf a -r ." == r"""
    fastannotate: a: using fast path (resolved fctx: True)
    0 a: a1
    1 b: a2
    3 b: b3
    3 b: b4"""

# annotate an ancient revision where the path matches. linelog can be used

sh % "hg fastannotate --debug -nf a -r 0" == r"""
    fastannotate: a: using fast path (resolved fctx: True)
    0 a: a1"""
sh % "hg fastannotate --debug -nf a -r 653e95416ebb5dbcc25bbc7f75568c9e01f7bd2f" == r"""
    fastannotate: a: using fast path (resolved fctx: False)
    0 a: a1"""
