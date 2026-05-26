
#require no-eden


Path conflict checking is currently disabled by default because of issue5716.
Turn it on for this test.

  $ setconfig experimental.merge.checkpathconflicts=True

  $ eagerepo

  $ sl init repo
  $ cd repo
  $ echo base > base
  $ sl add base
  $ sl commit -m "base"
  $ sl bookmark -i base
  $ mkdir a
  $ echo 1 > a/b
  $ sl add a/b
  $ sl commit -m "file"
  $ sl bookmark -i file
  $ echo 2 > a/b
  $ sl commit -m "file2"
  $ sl bookmark -i file2
  $ sl up 'desc(base)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
#if symlink
  $ mkdir a
  $ ln -s c a/b
  $ sl add a/b
  $ sl commit -m "link"
  $ sl bookmark -i link
  $ sl up 'desc(base)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
#endif
  $ mkdir -p a/b/c
  $ echo 2 > a/b/c/d
  $ sl add a/b/c/d
  $ sl commit -m "dir"
  $ sl bookmark -i dir

Merge - local file conflicts with remote directory

  $ sl up file
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark file)
#if symlink
  $ sl bookmark -i
  $ sl merge --verbose dir
  resolving manifests
  a/b: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a/b~029c48e05f7e
  resolve manually then use 'sl resolve --mark a/b'
  moving a/b to a/b~029c48e05f7e
  getting a/b/c/d
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl status
  M a/b/c/d
  A a/b~029c48e05f7e
  R a/b
  $ sl resolve --all
  a/b: path conflict must be resolved manually
  $ sl forget a/b~029c48e05f7e && rm a/b~029c48e05f7e
  $ sl resolve --mark a/b
  (no more unresolved files)
  $ sl commit -m "merge file and dir (deleted file)"

Merge - local symlink conflicts with remote directory

  $ sl up link
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark link)
  $ sl bookmark -i
  $ sl merge dir
  a/b: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a/b~f02dc228b64d
  resolve manually then use 'sl resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl status
  M a/b/c/d
  A a/b~f02dc228b64d
  R a/b
  $ sl resolve --list
  P a/b
  $ sl resolve --all
  a/b: path conflict must be resolved manually
  $ sl mv a/b~f02dc228b64d a/b.old
  $ sl resolve --mark a/b
  (no more unresolved files)
  $ sl resolve --list
  R a/b
  $ sl commit -m "merge link and dir (renamed link)"

Merge - local directory conflicts with remote file or link

  $ sl up dir
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark dir)
  $ sl bookmark -i
  $ sl merge file
  a/b: path conflict - a file or link has the same name as a directory
  the remote file has been renamed to a/b~029c48e05f7e
  resolve manually then use 'sl resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl status
  A a/b~029c48e05f7e
  $ sl resolve --all
  a/b: path conflict must be resolved manually
  $ sl mv a/b~029c48e05f7e a/b/old-b
  $ sl resolve --mark a/b
  (no more unresolved files)
  $ sl commit -m "merge dir and file (move file into dir)"
  $ sl merge file2
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat a/b/old-b
  1
  $ sl commit -m "merge file2 (copytrace tracked rename)"
  $ sl merge link
  a/b: path conflict - a file or link has the same name as a directory
  the remote file has been renamed to a/b~f02dc228b64d
  resolve manually then use 'sl resolve --mark a/b'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'sl resolve' to retry unresolved file merges or 'sl goto -C .' to abandon
  [1]
  $ sl mv a/b~f02dc228b64d a/b.old
#if no-windows
(On Windows, `sl mv` code path bypasses vfs, and might not treat symlink properly, needs investigation)
  $ f a/b.old
  a/b.old -> c
#endif
  $ sl resolve --mark a/b
  (no more unresolved files)
  $ sl commit -m "merge link (rename link)"
#endif
