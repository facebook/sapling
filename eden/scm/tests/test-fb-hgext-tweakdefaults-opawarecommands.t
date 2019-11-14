  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > amend=
  > rebase=
  > tweakdefaults=
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
  09d39afb522a08bdb03dc231608f7a3488ab4edc * 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'} (glob)

Check rebase metadata
  $ hg book -r . destination
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b
  adding b
  $ hg rebase -r . -d destination
  rebasing 1e9a3c00cbe9 "b" (tip)
  $ hg debugobsolete
  09d39afb522a08bdb03dc231608f7a3488ab4edc * 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'amend', 'user': 'test'} (glob)
  * * 0 (Thu Jan 01 00:00:00 1970 +0000) {'operation': 'rebase', 'user': 'test'} (glob)
