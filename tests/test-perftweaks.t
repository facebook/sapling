#if osx
#else
Test disabling the case conflict check (only fails on case sensitive systems)
  $ hg init casecheck
  $ cd casecheck
  $ cat >> .hg/hgrc <<EOF
  > [perftweaks]
  > disablecasecheck=True
  > EOF
  $ touch a
  $ hg add a
  $ hg commit -m a
  $ touch A
  $ hg add A
  warning: possible case-folding collision for A
  $ hg commit -m A
  $ cd ..
#endif

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

