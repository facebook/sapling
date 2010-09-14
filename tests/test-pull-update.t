  $ hg init t
  $ cd t
  $ echo 1 > foo
  $ hg ci -Am m
  adding foo

  $ cd ..
  $ hg clone t tt
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tt
  $ echo 1.1 > foo
  $ hg ci -Am m

  $ cd ../t
  $ echo 1.2 > foo
  $ hg ci -Am m

Should not update:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  not updating, since new heads added
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ cd ../tt

Should not update:

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  not updating, since new heads added
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ HGMERGE=true hg merge
  merging foo
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -mm

  $ cd ../t

Should work:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (-1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

