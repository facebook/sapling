Test disabling the tag cache
  $ hg init tagcache
  $ cd tagcache
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > blackbox=
  > EOF
  $ touch a && hg add -q a
  $ hg commit -qm "Foo"
  $ hg tag foo

  $ rm -rf .hg/cache .hg/blackbox.log
  $ hg tags
  tip                                1:2cc13e58bcd8
  foo                                0:be5a2292aa62
#if no-fsmonitor
  $ hg blackbox | grep tag
  *> tags (glob)
  *> writing * bytes to cache/hgtagsfnodes1 (glob)
  *> writing .hg/cache/tags2-visible with 1 tags (glob)
  *> tags exited 0 after * seconds (glob)
#endif

  $ rm -rf .hg/cache .hg/blackbox.log
  $ hg tags --config perftweaks.disabletags=True
  tip                                1:2cc13e58bcd8
  $ hg blackbox | grep tag
  *> tags* (glob)
  *> tags --config 'perftweaks.disabletags=True' exited 0 after * seconds (glob)

  $ cd ..

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

