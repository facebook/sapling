  $ hg init repo
  $ cd repo
  $ echo base > base
  $ hg add base
  $ hg commit -m "base"
  $ hg bookmark -i base
  $ echo 1 > a
  $ hg add a
  $ hg commit -m "file"
  $ hg bookmark -i file
  $ echo 2 > a
  $ hg commit -m "file2"
  $ hg bookmark -i file2
  $ hg up -q 0
  $ mkdir a
  $ echo 2 > a/b
  $ hg add a/b
  $ hg commit -m "dir"
  created new head
  $ hg bookmark -i dir

Basic merge - local file conflicts with remote directory

  $ hg up -q file
  $ hg bookmark -i
  $ hg merge --verbose dir
  resolving manifests
  a: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a~853701544ac3
  resolve manually then use 'hg resolve --mark a'
  moving a to a~853701544ac3
  getting a/b
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg update --clean .
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ rm a~853701544ac3

Basic update - local directory conflicts with remote file

  $ hg up -q 0
  $ mkdir a
  $ echo 3 > a/b
  $ hg up file
  a: untracked directory conflicts with file
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up --clean file
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark file)

Repo state is ok

  $ hg sum
  parent: 1:853701544ac3 
   file
  branch: default
  bookmarks: *file
  commit: (clean)
  update: 2 new changesets (update)
  phases: 4 draft

Basic update - untracked file conflicts with remote directory

  $ hg up -q 0
  $ echo untracked > a
  $ hg up --config merge.checkunknown=warn dir
  a: replacing untracked file
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark dir)
  $ cat a.orig
  untracked
  $ rm -f a.orig

Basic clean update - local directory conflicts with changed remote file

  $ hg up -q file
  $ rm a
  $ mkdir a
  $ echo 4 > a/b
  $ hg up file2
  abort: *: '$TESTTMP/repo/a' (glob)
  [255]
  $ hg up --clean file2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark file2)

Repo state is ok

  $ hg sum
  parent: 2:f64e09fac717 
   file2
  branch: default
  bookmarks: *file2
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)
  phases: 4 draft

