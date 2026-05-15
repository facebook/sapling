
#require no-eden

  $ setconfig commands.update.check=none

  $ HGMERGE=true; export HGMERGE

  $ eagerepo

  $ sl init r1 --config format.use-eager-repo=True
  $ cd r1
  $ echo a > a
  $ sl addremove
  adding a
  $ sl commit -m "1"
  $ sl book main

  $ newclientrepo r2 r1 main
  $ sl up tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo abc > a
  $ sl diff --nodates
  diff -r c19d34741b0a a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +abc

  $ cd ../r1
  $ echo b > b
  $ echo a2 > a
  $ sl addremove
  adding b
  $ sl commit -m "2"

  $ cd ../r2
  $ sl pull
  pulling from test:r1
  searching for changes
  $ sl status
  M a
  $ sl parents
  commit:      c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1

  $ sl --debug up 'desc(2)' --merge
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
  $ sl parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ sl --debug up 'desc(1)' --merge
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
  $ sl parents
  commit:      c19d34741b0a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ sl --debug up tip
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
  $ sl parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ sl -v history
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
  $ sl diff --nodates
  diff -r 1e71731e6fbb a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a2
  +abc


create a second head

  $ cd ../r1
  $ sl up 'desc(1)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark main)
  $ echo b2 > b
  $ echo a3 > a
  $ sl addremove
  adding b
  $ sl commit -m "3"

  $ cd ../r2
  $ sl -q pull ../r1
  $ sl status
  M a
  $ sl parents
  commit:      1e71731e6fbb
  bookmark:    remote/main
  hoistedname: main
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  $ sl --debug up main
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

test conflicting untracked files

  $ sl up -qC 'desc(1)'
  $ echo untracked > b
  $ sl st
  ? b
  $ sl up 'desc(2)'
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]
  $ rm b

test conflicting untracked ignored file

  $ sl up -qC 'desc(1)'
  $ echo ignored > .gitignore
  $ sl add .gitignore
  $ sl ci -m 'add .gitignore'
  $ echo ignored > ignored
  $ sl add ignored
  the following files are ignored, but still added because they are explicitly specified:
    ignored
  (use 'sl debugignore <file>' to check why they are ignored)
  $ sl ci -m 'add ignored file'

  $ sl up -q 'desc("add .gitignore")'
  $ echo untracked > ignored
  $ sl st
  $ sl up 'desc("add ignored file")'
  ignored: untracked file differs
  abort: untracked files in working directory differ from files in requested revision
  [255]

test a local add

  $ cd ..
  $ sl init a
  $ newclientrepo b a
  $ cd ..
  $ echo a > b/a
  $ echo a > a/a
  $ sl --cwd a commit -A -m a
  adding a
  $ sl --cwd a book main
  $ cd b
  $ sl add a
  $ sl pull -u -B main
  pulling from test:a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sl st

test updating backwards through a rename is not supported yet. `update` is not a
common use case for copy tracing, and enable copy tracing can impact the performance
for long distance update. Time will tell if we really need it.

  $ sl mv a b
  $ sl ci -m b
  $ echo b > b
  $ sl log -G -T '{node|short} {desc}' -p --git
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

  $ sl up -q 'desc(a)'
  local [working copy] changed b which other [destination] deleted
  use (c)hanged version, (d)elete, or leave (u)nresolved? u
  [1]
  $ sl st
  A b
  $ sl diff --nodates
  diff -r cb9a9f314b8b b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

test for superfluous filemerge of clean files renamed in the past

  $ sl up -qC tip
  $ echo c > c
  $ sl add c
  $ sl up -qt:fail 'desc(a)'

  $ cd ..
