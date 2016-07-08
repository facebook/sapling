  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/hgext3rd/tweakdefaults.py $TESTTMP # use $TESTTMP substitution in message
  $ cp $extpath/hgext3rd/fbamend.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > tweakdefaults=$TESTTMP/tweakdefaults.py
  > fbamend=$TESTTMP/fbamend.py
  > rebase=
  > [experimental]
  > evolution=createmarkers
  > EOF

Setup repo
  $ hg init opawarerepo
  $ cd opawarerepo
  $ echo root > root && hg ci -Am root
  adding root

Check amend metadata
  $ echo a > a && hg ci -Am a
  adding a
  $ echo aa > a && hg amend
  $ hg debugobsolete
  .* {'operation': 'amend', 'user': 'test'} (re)
  .* {'operation': 'amend', 'user': 'test'} (re)

Check rebase metadata
  $ hg book -r . destination
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b
  adding b
  created new head
  $ hg rebase -r . -d destination
  rebasing 4:1e9a3c00cbe9 "b" (tip)
  $ hg debugobsolete
  .* {'operation': 'amend', 'user': 'test'} (re)
  .* {'operation': 'amend', 'user': 'test'} (re)
  .* {'operation': 'rebase', 'user': 'test'} (re)
