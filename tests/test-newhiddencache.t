
  $ extpath=`dirname $TESTDIR`
  $ . $TESTDIR/require-ext.sh evolve
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > evolve=
  > newhiddencachekey=$extpath/hgext3rd/perftweaks.py
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
  > EOF

  $ hg init repo
  $ cd repo
  $ ls .hg/cache/hidden 2> /dev/null
  [2]
  $ hg debugbuilddag +1
  $ hg up -q 0
  $ ls .hg/cache/hidden 2> /dev/null
  [2]
  $ hg log -r . --debug | grep 'hidden cache'
  [1]
  $ ls .hg/cache/hidden 2> /dev/null
  [2]
  $ hg prune . --debug | grep 'hidden cache'
  recomputing hidden cache
  $ hg log -r . --debug | grep 'hidden cache'
  using hidden cache
  $ ls .hg/cache/hidden 2> /dev/null
  .hg/cache/hidden
