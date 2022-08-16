#chg-compatible
#debugruntest-compatible

  $ enable amend rebase tweakdefaults
  $ configure mutation-norecord

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

Check rebase metadata
  $ hg book -r . destination
  $ hg up 'desc(root)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b && hg ci -Am b
  adding b
  $ hg rebase -r . -d destination
  rebasing 1e9a3c00cbe9 "b"
  $ hg debugobsolete
