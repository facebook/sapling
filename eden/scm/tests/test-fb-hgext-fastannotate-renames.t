#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#require py2

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > fastannotate=
  > [fastannotate]
  > mainbranch=main
  > EOF

  $ hg init repo
  $ cd repo

# add or rename files on top of the master branch

  $ echo a1 > a
  $ echo b1 > b
  $ hg commit -qAm 1
  $ hg bookmark -i main
  $ hg fastannotate --debug -nf b
  fastannotate: b: 1 new changesets in the main branch
  0 b: b1
  $ hg fastannotate --debug -nf a
  fastannotate: a: 1 new changesets in the main branch
  0 a: a1
  $ echo a2 >> a
  $ cat > b << 'EOF'
  > b0
  > b1
  > EOF
  $ hg mv a t
  $ hg mv b a
  $ hg mv t b
  $ hg commit -m 'swap names'

# existing linelogs are not helpful with such renames in side branches

  $ hg fastannotate --debug -nf a
  fastannotate: a: linelog cannot help in annotating this revision
  1 a: b0
  0 b: b1
  $ hg fastannotate --debug -nf b
  fastannotate: b: linelog cannot help in annotating this revision
  0 a: a1
  1 b: a2

# move main branch forward, rebuild should happen

  $ hg bookmark -i main -r . -q
  $ hg fastannotate --debug -nf b
  fastannotate: b: cache broken and deleted
  fastannotate: b: 2 new changesets in the main branch
  0 a: a1
  1 b: a2
  $ hg fastannotate --debug -nf b
  fastannotate: b: using fast path (resolved fctx: True)
  0 a: a1
  1 b: a2

# for rev 0, the existing linelog is still useful for a, but not for b

  $ hg fastannotate --debug -nf a -r 0
  fastannotate: a: using fast path (resolved fctx: True)
  0 a: a1
  $ hg fastannotate --debug -nf b -r 0
  fastannotate: b: linelog cannot help in annotating this revision
  0 b: b1

# a rebuild can also be triggered if "the main branch last time" mismatches

  $ echo a3 >> a
  $ hg commit -m a3
  $ cat >> b << 'EOF'
  > b3
  > b4
  > EOF
  $ hg commit -m b4
  $ hg bookmark -i main -q
  $ hg fastannotate --debug -nf a
  fastannotate: a: cache broken and deleted
  fastannotate: a: 3 new changesets in the main branch
  1 a: b0
  0 b: b1
  2 a: a3
  $ hg fastannotate --debug -nf a
  fastannotate: a: using fast path (resolved fctx: True)
  1 a: b0
  0 b: b1
  2 a: a3

# linelog can be updated without being helpful

  $ hg mv a t
  $ hg mv b a
  $ hg mv t b
  $ hg commit -m 'swap names again'
  $ hg fastannotate --debug -nf b
  fastannotate: b: 1 new changesets in the main branch
  1 a: b0
  0 b: b1
  2 a: a3
  $ hg fastannotate --debug -nf b
  fastannotate: b: linelog cannot help in annotating this revision
  1 a: b0
  0 b: b1
  2 a: a3

# move main branch forward again, rebuilds are one-time

  $ hg bookmark -i main -q
  $ hg fastannotate --debug -nf a
  fastannotate: a: cache broken and deleted
  fastannotate: a: 4 new changesets in the main branch
  0 a: a1
  1 b: a2
  3 b: b3
  3 b: b4
  $ hg fastannotate --debug -nf b
  fastannotate: b: cache broken and deleted
  fastannotate: b: 4 new changesets in the main branch
  1 a: b0
  0 b: b1
  2 a: a3
  $ hg fastannotate --debug -nf a
  fastannotate: a: using fast path (resolved fctx: True)
  0 a: a1
  1 b: a2
  3 b: b3
  3 b: b4
  $ hg fastannotate --debug -nf b
  fastannotate: b: using fast path (resolved fctx: True)
  1 a: b0
  0 b: b1
  2 a: a3

# list changeset hashes to improve readability

  $ hg log -T '{rev}:{node}\n'
  4:980e1ab8c516350172928fba95b49ede3b643dca
  3:14e123fedad9f491f5dde0beca2a767625a0a93a
  2:96495c41e4c12218766f78cdf244e768d7718b0f
  1:35c2b781234c994896aba36bd3245d3104e023df
  0:653e95416ebb5dbcc25bbc7f75568c9e01f7bd2f

# annotate a revision not in the linelog. linelog cannot be used, but does not get rebuilt either

  $ hg fastannotate --debug -nf a -r 96495c41e4c12218766f78cdf244e768d7718b0f
  fastannotate: a: linelog cannot help in annotating this revision
  1 a: b0
  0 b: b1
  2 a: a3
  $ hg fastannotate --debug -nf a -r 2
  fastannotate: a: linelog cannot help in annotating this revision
  1 a: b0
  0 b: b1
  2 a: a3
  $ hg fastannotate --debug -nf a -r .
  fastannotate: a: using fast path (resolved fctx: True)
  0 a: a1
  1 b: a2
  3 b: b3
  3 b: b4

# annotate an ancient revision where the path matches. linelog can be used

  $ hg fastannotate --debug -nf a -r 0
  fastannotate: a: using fast path (resolved fctx: True)
  0 a: a1
  $ hg fastannotate --debug -nf a -r 653e95416ebb5dbcc25bbc7f75568c9e01f7bd2f
  fastannotate: a: using fast path (resolved fctx: False)
  0 a: a1
