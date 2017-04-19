Set up test environment.
  $ . $TESTDIR/require-ext.sh evolve
  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > evolve=
  > fbamend=$TESTTMP/fbamend.py
  > rebase=
  > [experimental]
  > evolution = createmarkers
  > evolutioncommands = prev next split fold
  > EOF
  $ hg init repo && cd repo

Perform restack without inhibit extension.
  $ hg debugbuilddag -m +3
  $ hg update 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg amend -m "Amended"
  warning: the changeset's children were left behind
  (use 'hg rebase --restack' (alias: 'hg restack') to rebase them)
  $ hg rebase --restack
  rebasing 2:* "r2" (glob)
