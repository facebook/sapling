  $ hg init t
  $ cd t
  $ echo 1 > foo
  $ hg ci -Am1 # 0
  adding foo
  $ hg branch branchA
  marked working directory as branch branchA
  $ echo a1 > foo
  $ hg ci -ma1 # 1

  $ cd ..
  $ hg init tt
  $ cd tt
  $ hg pull ../t
  pulling from ../t
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up branchA
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../t
  $ echo a2 > foo
  $ hg ci -ma2 # 2

Create branch B:

  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch branchB
  marked working directory as branch branchB
  $ echo b1 > foo
  $ hg ci -mb1 # 3

  $ cd ../tt

A new branch is there

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files (+1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Develop both branches:

  $ cd ../t
  $ hg up branchA
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a3 > foo
  $ hg ci -ma3 # 4
  $ hg up branchB
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b2 > foo
  $ hg ci -mb2 # 5

  $ cd ../tt

Should succeed, no new heads:

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Add a head on other branch:

  $ cd ../t
  $ hg up branchA
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a4 > foo
  $ hg ci -ma4 # 6
  $ hg up branchB
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b3.1 > foo
  $ hg ci -m b3.1 # 7
  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b3.2 > foo
  $ hg ci -m b3.2 # 8
  created new head

  $ cd ../tt

Should succeed because there is only one head on our branch:

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files (+1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd ../t
  $ hg up -C branchA
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a5.1 > foo
  $ hg ci -ma5.1 # 9
  $ hg up 6
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a5.2 > foo
  $ hg ci -ma5.2 # 10
  created new head
  $ hg up 7
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b4.1 > foo
  $ hg ci -m b4.1 # 11
  $ hg up -C 8
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b4.2 > foo
  $ hg ci -m b4.2 # 12

  $ cd ../tt

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files (+1 heads)
  not updating, since new heads added
  (run 'hg heads' to see heads, 'hg merge' to merge)

