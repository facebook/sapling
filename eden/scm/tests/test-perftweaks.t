#debugruntest-compatible

Test avoiding calculating head changes during commit

  $ hg init branchatcommit
  $ cd branchatcommit
  $ hg debugdrawdag<<'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up -q A
  $ echo C > C
  $ hg commit -m C -A C
  $ hg up -q A
  $ echo D > D
  $ hg commit -m D -A D

