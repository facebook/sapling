
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > perftweaks=
  > EOF

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

Test disabling resolving non-default branch names

  $ hg init branchresolve
  $ cd branchresolve
  $ echo 1 >> A
  $ hg commit -A A -m 1
  $ hg branch foo
  marked working directory as branch foo
  (branches are permanent and global, did you want a bookmark?)
  $ echo 2 >> A
  $ hg commit -A A -m 2
  $ hg log -r default -T '{desc}\n'
  1
  $ hg log -r foo -T '{desc}\n'
  2
  $ hg log -r default -T '{desc}\n' --config perftweaks.disableresolvingbranches=1
  1
  $ hg log -r foo -T '{desc}\n' --config perftweaks.disableresolvingbranches=1
  abort: unknown revision 'foo'!
  [255]
  $ cd ..

Test disabling the branchcache
  $ hg init branchcache
  $ cd branchcache
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > blackbox=
  > strip=
  > EOF
  $ echo a > a
  $ hg commit -Aqm a
#if no-fsmonitor
  $ hg blackbox
  *> commit -Aqm a (glob)
  *> updated served branch cache in * seconds (glob)
  *> wrote served branch cache with 1 labels and 1 nodes (glob)
  *> commit -Aqm a exited 0 after * seconds (glob)
  *> blackbox (glob)
#endif
  $ hg strip -q -r . -k
  $ rm .hg/blackbox.log
  $ rm -rf .hg/cache
  $ hg commit -Aqm a --config perftweaks.disablebranchcache=True --config perftweaks.disablebranchcache2=True
#if no-fsmonitor
  $ hg blackbox
  *> commit -Aqm a* (glob)
  *> perftweaks updated served branch cache (glob)
  *> commit -Aqm a * exited 0 after * seconds (glob)
  *> blackbox (glob)
#endif

  $ cd ..

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

Test disabling updating branchcache during commit

  $ $TESTDIR/ls-l.py .hg/cache | grep branch
  -rw-r--r--     196 branch2-served

  $ rm -f .hg/cache/branch*
  $ echo D >> D
  $ hg commit -m D2
  $ $TESTDIR/ls-l.py .hg/cache | grep branch
  -rw-r--r--     196 branch2-served

  $ rm -f .hg/cache/branch*
  $ echo D >> D
  $ hg commit -m D3 --config perftweaks.disableupdatebranchcacheoncommit=1 --config perftweaks.disableheaddetection=1
  $ $TESTDIR/ls-l.py .hg/cache | grep branch
  [1]

  $ cd ..
