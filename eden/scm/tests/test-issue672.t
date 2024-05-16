
#require no-eden


  $ eagerepo
https://bz.mercurial-scm.org/672

# 0-2-4
#  \ \ \
#   1-3-5
#
# rename in #1, content change in #4.

  $ hg init repo
  $ cd repo

  $ touch 1
  $ touch 2
  $ hg commit -Am init  # 0
  adding 1
  adding 2

  $ hg rename 1 1a
  $ hg commit -m rename # 1

  $ hg co -C 'desc(init)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo unrelated >> 2
  $ hg ci -m unrelated1 # 2

  $ hg merge --debug 'desc(rename)'
  resolving manifests
   branchmerge: True, force: False
   ancestor: 81f4b099af3d, local: c64f439569a9+, remote: c12dcd37c90a
   1: other deleted -> r
  removing 1
   1a: remote created -> g
  getting 1a
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m merge1 # 3

  $ hg co -C 'desc(unrelated1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo hello >> 1
  $ hg ci -m unrelated2 # 4

  $ hg co -C 'desc(merge1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log -G -T '{node|short} {desc}'
  o  746e9549ea96 unrelated2
  │
  │ @  e327dca35ac8 merge1
  ╭─┤
  o │  c64f439569a9 unrelated1
  │ │
  │ o  c12dcd37c90a rename
  ├─╯
  o  81f4b099af3d init
  $ hg log -r 'p1(e327dca35ac8)' -T '{node|short} {desc}\n'
  c64f439569a9 unrelated1

# dagcopytrace does not support merge commits (it only searches p1)

  $ hg merge -y --debug 'desc(unrelated2)'
  resolving manifests
   branchmerge: True, force: False
   ancestor: c64f439569a9, local: e327dca35ac8+, remote: 746e9549ea96
   1: prompt deleted/changed -> m (premerge)
  picktool() hgmerge :prompt internal:merge
  picked tool ':prompt' for path=1 binary=False symlink=False changedelete=True
  other [merge rev] changed 1 which local [working copy] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

# dagcopytrace does not support merge commits (it only searches p1)

  $ hg co -C 'desc(unrelated2)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge -y --debug 'desc(merge1)'
  resolving manifests
   branchmerge: True, force: False
   ancestor: c64f439569a9, local: 746e9549ea96+, remote: e327dca35ac8
   preserving 1 for resolve of 1
   1a: remote created -> g
  getting 1a
   1: prompt changed/deleted -> m (premerge)
  picktool() hgmerge :prompt internal:merge
  picked tool ':prompt' for path=1 binary=False symlink=False changedelete=True
  local [working copy] changed 1 which other [merge rev] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
