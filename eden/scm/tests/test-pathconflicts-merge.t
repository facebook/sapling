#chg-compatible
#debugruntest-compatible

#require symlink

  $ setconfig workingcopy.ruststatus=False
Path conflict checking is currently disabled by default because of issue5716.
Turn it on for this test.

  $ setconfig experimental.merge.checkpathconflicts=True

  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"
  $ hg bookmark -i base
  $ mkdir a
  $ echo 1 > a/b
  $ hg add a/b
  $ hg commit -m "file"
  $ hg bookmark -i file
  $ echo 2 > a/b
  $ hg commit -m "file2"
  $ hg bookmark -i file2
  $ hg up 'desc(base)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir a
  $ ln -s c a/b
  $ hg add a/b
  $ hg commit -m "link"
  $ hg bookmark -i link
  $ hg up 'desc(base)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir -p a/b/c
  $ echo 2 > a/b/c/d
  $ hg add a/b/c/d
  $ hg commit -m "dir"
  $ hg bookmark -i dir

Merge - local file conflicts with remote directory

  $ hg up file
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark file)
  $ hg bookmark -i
  $ hg merge --verbose dir
  resolving manifests
  a/b: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a/b~029c48e05f7e
  resolve manually then use 'hg resolve --mark a/b'
  moving a/b to a/b~029c48e05f7e
  getting a/b/c/d
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg status
  M a/b/c/d
  A a/b~029c48e05f7e
  R a/b
  $ hg resolve --all
  a/b: path conflict must be resolved manually
  $ hg forget a/b~029c48e05f7e && rm a/b~029c48e05f7e
  $ hg resolve --mark a/b
  (no more unresolved files)
  $ hg commit -m "merge file and dir (deleted file)"

Merge - local symlink conflicts with remote directory

  $ hg up link
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark link)
  $ hg bookmark -i
  $ hg merge dir
  a/b: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a/b~f02dc228b64d
  resolve manually then use 'hg resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg status
  M a/b/c/d
  A a/b~f02dc228b64d
  R a/b
  $ hg resolve --list
  P a/b
  $ hg resolve --all
  a/b: path conflict must be resolved manually
  $ hg mv a/b~f02dc228b64d a/b.old
  $ hg resolve --mark a/b
  (no more unresolved files)
  $ hg resolve --list
  R a/b
  $ hg commit -m "merge link and dir (renamed link)"

Merge - local directory conflicts with remote file or link

  $ hg up dir
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark dir)
  $ hg bookmark -i
  $ hg merge file
  a/b: path conflict - a file or link has the same name as a directory
  the remote file has been renamed to a/b~029c48e05f7e
  resolve manually then use 'hg resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg status
  A a/b~029c48e05f7e
  $ hg resolve --all
  a/b: path conflict must be resolved manually
  $ hg mv a/b~029c48e05f7e a/b/old-b
  $ hg resolve --mark a/b
  (no more unresolved files)
  $ hg commit -m "merge dir and file (move file into dir)"
  $ hg merge file2
  merging a/b/old-b and a/b to a/b/old-b
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat a/b/old-b
  2
  $ hg commit -m "merge file2 (copytrace tracked rename)"
  $ hg merge link
  a/b: path conflict - a file or link has the same name as a directory
  the remote file has been renamed to a/b~f02dc228b64d
  resolve manually then use 'hg resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg mv a/b~f02dc228b64d a/b.old
  $ f a/b.old
  a/b.old -> c
  $ hg resolve --mark a/b
  (no more unresolved files)
  $ hg commit -m "merge link (rename link)"
