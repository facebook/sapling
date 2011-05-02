#  A          B
#
#  3  4       3
#  |\/|       |\
#  |/\|       | \
#  1  2       1  2
#  \ /        \ /
#   0          0
#
# if the result of the merge of 1 and 2
# is the same in 3 and 4, no new manifest
# will be created and the manifest group
# will be empty during the pull
#
# (plus we test a failure where outgoing
# wrongly reported the number of csets)

  $ hg init a
  $ cd a
  $ touch init
  $ hg ci -A -m 0
  adding init
  $ touch x y
  $ hg ci -A -m 1
  adding x
  adding y

  $ hg update 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ touch x y
  $ hg ci -A -m 2
  adding x
  adding y
  created new head

  $ hg merge 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -A -m m1

  $ hg update -C 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -A -m m2
  created new head

  $ cd ..

  $ hg clone -r 3 a b
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg clone -r 4 a c
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 3 changes to 3 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg -R a outgoing b
  comparing with b
  searching for changes
  changeset:   4:1ec3c74fc0e0
  tag:         tip
  parent:      1:79f9e10cd04e
  parent:      2:8e1bb01c1a24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m2
  
  $ hg -R a outgoing c
  comparing with c
  searching for changes
  changeset:   3:d15a0c284984
  parent:      2:8e1bb01c1a24
  parent:      1:79f9e10cd04e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m1
  
  $ hg -R b outgoing c
  comparing with c
  searching for changes
  changeset:   3:d15a0c284984
  tag:         tip
  parent:      2:8e1bb01c1a24
  parent:      1:79f9e10cd04e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m1
  
  $ hg -R c outgoing b
  comparing with b
  searching for changes
  changeset:   3:1ec3c74fc0e0
  tag:         tip
  parent:      1:79f9e10cd04e
  parent:      2:8e1bb01c1a24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     m2
  

  $ hg -R b pull a
  pulling from a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg -R c pull a
  pulling from a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
