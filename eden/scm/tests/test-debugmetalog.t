#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig "visibility.enabled=true" "experimental.narrow-heads=1" "remotenames.selectivepull=1" "mutation.date=0 0" "metalog.track-config=0"

  $ newrepo
  $ enable remotenames amend

  $ hg debugdrawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ hg up -q 'desc(A)'
  $ hg bookmark foo
  $ hg up -q 'desc(C)'
  $ hg bookmark foo >/dev/null

  $ hg debugdrawdag << 'EOS'
  > E
  > |
  > D
  > |
  > desc(B)
  > EOS

  $ hg up -q 'desc(E)'
  $ hg metaedit -mE1

  $ hg debugmetalog
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
     1970-01-01 00:00:00 +0000: foo (removed by bookmark foo)

  $ hg debugmetalogroots -v | tee out
      6 1970-01-01 00:00:00 +0000 6db96a1ccb768e6ca28112ec49956b8f26ac4265 metaedit -mE1 Parent: 66a7c5ab3f9e57bafd8754793ea7e2d8876...
      5 1970-01-01 00:00:00 +0000 66a7c5ab3f9e57bafd8754793ea7e2d8876e8930 debugdrawdag Parent: f2f1b378abd71e829b4d7bd6fcb801affed7...
      4 1970-01-01 00:00:00 +0000 f2f1b378abd71e829b4d7bd6fcb801affed734b3 bookmark foo Parent: 21f308932829c1b8bc31deba6f05fa2e1a0c...
      3 1970-01-01 00:00:00 +0000 21f308932829c1b8bc31deba6f05fa2e1a0cecc5 bookmark foo Parent: 8955280c49fbefa5bf1e539d76f81c7502f7...
      2 1970-01-01 00:00:00 +0000 8955280c49fbefa5bf1e539d76f81c7502f73987 debugdrawdag Parent: e0c47396402d4bbc0eb4f8672ada4951ebc0...
      1 1970-01-01 00:00:00 +0000 e0c47396402d4bbc0eb4f8672ada4951ebc09dc6 init tracked
      0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d 

  $ hg up -q null

  $ HGFORCEMETALOGROOT=$(grep debugdrawdag out | head -1 | sed 's/.*\+0000 (.{40}) debugdrawdag.*/\1/') hg log -G -r 'all()' -T '{desc} {bookmarks}'
  o  E E
  │
  o  D D
  │
  │ o  C C foo
  ├─╯
  o  B B
  │
  o  A A

  $ hg debugcompactmetalog

  $ hg debugmetalogroots -v
      1 1970-01-01 00:00:00 +0000 6db96a1ccb768e6ca28112ec49956b8f26ac4265 metaedit -mE1 Parent: 66a7c5ab3f9e57bafd8754793ea7e2d8876...
      0 1970-01-01 00:00:00 +0000 29e2dcfbb16f63bb0254df7585a15bb6fb5e927d 
