#chg-compatible

Path conflict checking is currently disabled by default because of issue5716.
Turn it on for this test.

  $ cat >> $HGRCPATH << EOF
  > [experimental]
  > merge.checkpathconflicts=True
  > EOF

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
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir a
#if symlink
  $ ln -s c a/b
#else
  $ touch a/b
#endif
  $ hg add a/b
  $ hg commit -m "link"
  $ hg bookmark -i link
  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ mkdir -p a/b/c
  $ echo 2 > a/b/c/d
  $ hg add a/b/c/d
  $ hg commit -m "dir"
  $ hg bookmark -i dir

Update - local file conflicts with remote directory:

  $ hg up -q 0
  $ mkdir a
  $ echo 9 > a/b
  $ hg up dir
  a/b: untracked file conflicts with directory
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up dir --config merge.checkunknown=warn
  a/b: replacing untracked file
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark dir)
  $ cat a/b.orig
  9
  $ rm a/b.orig

Update - local symlink conflicts with remote directory:

  $ hg up -q 0
  $ mkdir a
#if symlink
  $ ln -s x a/b
#else
  $ touch a/b
#endif
  $ hg up dir
  a/b: untracked file conflicts with directory
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up dir --config merge.checkunknown=warn
  a/b: replacing untracked file
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark dir)
#if symlink
  $ readlink.py a/b.orig
  a/b.orig -> x
#endif
  $ rm a/b.orig

Update - local directory conflicts with remote file

  $ hg up -q 0
  $ mkdir -p a/b/c
  $ echo 9 > a/b/c/d
  $ hg up file
  a/b: untracked directory conflicts with file
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up file --config merge.checkunknown=warn
  a/b: replacing untracked files in directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark file)
  $ cat a/b
  1
  $ test -d a/b.orig
  $ rm -rf a/b.orig

Update - local directory conflicts with remote symlink

  $ hg up -q 0
  $ mkdir -p a/b/c
  $ echo 9 > a/b/c/d
  $ hg up link
  a/b: untracked directory conflicts with file
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ hg up link --config merge.checkunknown=warn
  a/b: replacing untracked files in directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark link)
#if symlink
  $ readlink.py a/b
  a/b -> c
#endif
  $ test -d a/b.orig
  $ rm -rf a/b.orig

Update - local renamed file conflicts with remote directory

  $ hg up -q 0
  $ hg mv base a
  $ hg status -C
  A a
    base
  R base
  $ hg up --check dir
  abort: uncommitted changes
  [255]
  $ hg up dir
  a: path conflict - a file or link has the same name as a directory
  the local file has been renamed to a~d20a80d4def3
  resolve manually then use 'hg resolve --mark a'
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges
  (activating bookmark dir)
  [1]
  $ hg status -C
  A a~d20a80d4def3
    base
  R base
  $ hg resolve --list
  P a
  $ hg up --clean -q 0

Update clean - local directory conflicts with changed remote file

  $ hg up -q file
  $ rm a/b
  $ mkdir a/b
  $ echo 9 > a/b/c
  $ hg up file2 --check --config merge.checkunknown=warn
  abort: uncommitted changes
  [255]
  $ hg up file2 --clean
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark file2)
