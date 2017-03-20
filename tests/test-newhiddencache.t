
  $ extpath=`dirname $TESTDIR`
  $ . $TESTDIR/require-ext.sh evolve
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > evolve=
  > perftweaks=$extpath/hgext3rd/perftweaks.py
  > [experimental]
  > evolution=createmarkers
  > evolutioncommands=obsolete
  > EOF

  $ hg init repo
  $ cd repo
  $ ls .hg/ | grep cache
  [1]
  $ hg debugbuilddag +1
  $ hg up -q 0
  $ ls .hg/cache/ | grep hidden
  [1]
  $ hg log -r . --debug | grep 'hidden cache'
  [1]
  $ ls .hg/cache/ | grep hidden
  [1]
  $ hg prune . --debug | grep 'hidden cache'
  recomputing hidden cache
  $ hg log -r . --debug | grep 'hidden cache'
  using hidden cache
  $ ls .hg/cache/hidden 2> /dev/null
  .hg/cache/hidden
