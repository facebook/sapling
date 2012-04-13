bundle w/o type option

  $ hg init t1
  $ hg init t2
  $ cd t1
  $ echo blablablablabla > file.txt
  $ hg ci -Ama
  adding file.txt
  $ hg log | grep summary
  summary:     a
  $ hg bundle ../b1 ../t2
  searching for changes
  1 changesets found

  $ cd ../t2
  $ hg pull ../b1
  pulling from ../b1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log | grep summary
  summary:     a
  $ cd ..

test bundle types

  $ for t in "None" "bzip2" "gzip"; do
  >   echo % test bundle type $t
  >   hg init t$t
  >   cd t1
  >   hg bundle -t $t ../b$t ../t$t
  >   cut -b 1-6 ../b$t | head -n 1
  >   cd ../t$t
  >   hg pull ../b$t
  >   hg up
  >   hg log | grep summary
  >   cd ..
  > done
  % test bundle type None
  searching for changes
  1 changesets found
  HG10UN
  pulling from ../bNone
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  summary:     a
  % test bundle type bzip2
  searching for changes
  1 changesets found
  HG10BZ
  pulling from ../bbzip2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  summary:     a
  % test bundle type gzip
  searching for changes
  1 changesets found
  HG10GZ
  pulling from ../bgzip
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  summary:     a

test garbage file

  $ echo garbage > bgarbage
  $ hg init tgarbage
  $ cd tgarbage
  $ hg pull ../bgarbage
  abort: ../bgarbage: not a Mercurial bundle
  [255]
  $ cd ..

test invalid bundle type

  $ cd t1
  $ hg bundle -a -t garbage ../bgarbage
  abort: unknown bundle type specified with --type
  [255]
  $ cd ..
