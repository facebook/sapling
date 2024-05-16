
#require no-eden

  $ setconfig experimental.allowfilepeer=True
  $ setconfig commands.update.check=none

  $ HGMERGE=true; export HGMERGE

  $ eagerepo

  $ hg init r1 --config format.use-eager-repo=True
  $ cd r1
  $ echo a > a
  $ hg addremove
  adding a
  $ hg commit -m "1"
  $ hg book main

  $ newclientrepo r2 ~/r1 main
  $ hg up tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo abc > a
  $ hg diff --nodates
  diff -r c19d34741b0a a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +abc

  $ cd ../r1
  $ echo b > b
  $ echo a2 > a
  $ hg addremove
  adding b
  $ hg commit -m "2"

  $ cd ../r2
  $ hg pull
  pulling from $TESTTMP/r1
  searching for changes
  $ hg status
  M a
  $ hg parents
  commit:      c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1

  $ hg --debug up 'desc(2)' --merge
  resolving manifests
   branchmerge: False, force: False
   ancestor: c19d34741b0a, local: c19d34741b0a+, remote: 1e71731e6fbb
   preserving a for resolve of a
   a: versions differ -> m (premerge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
   a: versions differ -> m (merge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
  launching merge tool: true *$TESTTMP/r2/a* * * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ hg --debug up 'desc(1)' --merge
  resolving manifests
   branchmerge: False, force: False
   ancestor: 1e71731e6fbb, local: 1e71731e6fbb+, remote: c19d34741b0a
   preserving a for resolve of a
   a: versions differ -> m (premerge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  merging a
  my a@1e71731e6fbb+ other a@c19d34741b0a ancestor a@1e71731e6fbb
   a: versions differ -> m (merge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  my a@1e71731e6fbb+ other a@c19d34741b0a ancestor a@1e71731e6fbb
  launching merge tool: true *$TESTTMP/r2/a* * * (glob)
  merge tool returned: 0
  0 files updated, 1 files merged, 1 files removed, 0 files unresolved
  $ hg parents
  commit:      c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg --debug up tip
  resolving manifests
   branchmerge: False, force: False
   ancestor: c19d34741b0a, local: c19d34741b0a+, remote: 1e71731e6fbb
   preserving a for resolve of a
   a: versions differ -> m (premerge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  merging a
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
   a: versions differ -> m (merge)
  picktool() hgmerge true
  picked tool 'true' for path=a binary=False symlink=False changedelete=False
  my a@c19d34741b0a+ other a@1e71731e6fbb ancestor a@c19d34741b0a
  launching merge tool: true *$TESTTMP/r2/a* * * (glob)
  merge tool returned: 0
  1 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ hg -v history
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a b
  description:
  2
  
  
  commit:      c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  1
  $ hg diff --nodates
  diff -r 1e71731e6fbb a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a2
  +abc


create a second head

  $ cd ../r1
  $ hg up 'desc(1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark main)
  $ echo b2 > b
  $ echo a3 > a
  $ hg addremove
  adding b
  $ hg commit -m "3"

  $ cd ../r2
  $ hg -q pull ../r1
  $ hg status
  M a
  $ hg parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ hg --debug up main
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

test conflicting untracked files

  $ hg up -qC 'desc(1)'
  $ echo untracked > b
  $ hg st
  ? b
  $ hg up 'desc(2)'
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ rm b

test conflicting untracked ignored file

  $ hg up -qC 'desc(1)'
  $ echo ignored > .gitignore
  $ hg add .gitignore
  $ hg ci -m 'add .gitignore'
  $ echo ignored > ignored
  $ hg add ignored
  $ hg ci -m 'add ignored file'

  $ hg up -q 'desc("add .gitignore")'
  $ echo untracked > ignored
  $ hg st
  $ hg up 'desc("add ignored file")'
  ignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

test a local add

  $ cd ..
  $ hg init a
  $ newclientrepo b ~/a
  $ cd ..
  $ echo a > b/a
  $ echo a > a/a
  $ hg --cwd a commit -A -m a
  adding a
  $ hg --cwd a book main
  $ cd b
  $ hg add a
  $ hg pull -u -B main
  pulling from $TESTTMP/a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg st

test updating backwards through a rename is not supported yet. `update` is not a
common use case for copy tracing, and enable copy tracing can impact the performance
for long distance update. Time will tell if we really need it.

  $ hg mv a b
  $ hg ci -m b
  $ echo b > b
  $ hg log -G -T '{node|short} {desc}' -p --git
  @  fdbc53b96b17 bdiff --git a/a b/b
  │  rename from a
  │  rename to b
  │
  o  cb9a9f314b8b adiff --git a/a b/a
     new file mode 100644
     --- /dev/null
     +++ b/a
     @@ -0,0 +1,1 @@
     +a

For update, base=fdbc53b96b17, src=cb9a9f314b8b, dst=fdbc53b96b17

  $ hg up -q 'desc(a)'
  local [working copy] changed b which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  [1]
  $ hg st
  A b
  $ hg diff --nodates
  diff -r cb9a9f314b8b b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

test for superfluous filemerge of clean files renamed in the past

  $ hg up -qC tip
  $ echo c > c
  $ hg add c
  $ hg up -qt:fail 'desc(a)'

  $ cd ..
