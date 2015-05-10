http://mercurial.selenic.com/bts/issue672

# 0-2-4
#  \ \ \
#   1-3-5
#
# rename in #1, content change in #4.

  $ hg init

  $ touch 1
  $ touch 2
  $ hg commit -Am init  # 0
  adding 1
  adding 2

  $ hg rename 1 1a
  $ hg commit -m rename # 1

  $ hg co -C 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo unrelated >> 2
  $ hg ci -m unrelated1 # 2
  created new head

  $ hg merge --debug 1
    searching for copies back to rev 1
    unmatched files in other:
     1a
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: '1' -> dst: '1a' 
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 81f4b099af3d, local: c64f439569a9+, remote: c12dcd37c90a
   1: other deleted -> r
  removing 1
   1a: remote created -> g
  getting 1a
   2: remote unchanged -> k
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m merge1 # 3

  $ hg co -C 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo hello >> 1
  $ hg ci -m unrelated2 # 4
  created new head

  $ hg co -C 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge -y --debug 4
    searching for copies back to rev 1
    unmatched files in local:
     1a
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: '1' -> dst: '1a' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: c64f439569a9, local: e327dca35ac8+, remote: 746e9549ea96
   preserving 1a for resolve of 1a
   1a: local copied/moved from 1 -> m
  picked tool 'internal:merge' for 1a (binary False symlink False)
  merging 1a and 1 to 1a
  my 1a@e327dca35ac8+ other 1@746e9549ea96 ancestor 1@81f4b099af3d
   premerge successful
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg co -C 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge -y --debug 3
    searching for copies back to rev 1
    unmatched files in other:
     1a
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: '1' -> dst: '1a' *
    checking for directory renames
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: c64f439569a9, local: 746e9549ea96+, remote: e327dca35ac8
   preserving 1 for resolve of 1a
  removing 1
   1a: remote moved from 1 -> m
  picked tool 'internal:merge' for 1a (binary False symlink False)
  merging 1 and 1a to 1a
  my 1a@746e9549ea96+ other 1a@e327dca35ac8 ancestor 1@81f4b099af3d
   premerge successful
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

