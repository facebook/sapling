  $ hg init

  $ echo a > a
  $ hg ci -qAm 'add a'

  $ echo b > b
  $ hg ci -qAm 'add b'

  $ hg up -qC 0
  $ hg rm a
  $ hg ci -m 'rm a'
  created new head

  $ hg up -qC 1
  $ rm a

Local deleted a file, remote removed

Should fail, since there are deleted files:

  $ hg merge
  abort: uncommitted changes
  (use 'hg status' to list changes)
  [255]

Should succeed with --force:

  $ hg -v merge --force
  resolving manifests
  removing a
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Should show 'a' as removed:

  $ hg status
  R a

  $ hg ci -m merge

Should not show 'a':

  $ hg manifest
  b

