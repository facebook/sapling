  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x

  $ cd ..

  $ hgcloneshallow ssh://localhost/$PWD/master shallow -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ hgcloneshallow ssh://localhost/$PWD/master shallow2 -q

# pull to shallow from full

  $ cd master
  $ echo y > y
  $ hg commit -qAm y

  $ cd ../shallow
  $ hg pull
  pulling from ssh://localhost/$TESTTMP/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)

  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)

  $ cat y
  y

  $ cd ..

# pull from shallow to shallow

  $ cd shallow
  $ echo z > z
  $ hg commit -qAm z
  $ cd ../shallow2
  $ hg pull ssh://localhost//$TESTTMP/shallow
  pulling from ssh://localhost//$TESTTMP/shallow
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat z
  z

  $ cd ..

# push from shallow to shallow

  $ cd shallow
  $ echo a > a
  $ hg commit -qAm a
  $ hg push ssh://localhost//$TESTTMP/shallow2
  pushing to ssh://localhost//$TESTTMP/shallow2
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ cd ../shallow2
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a

  $ cd ..

# push from shallow to full

  $ cd shallow
  $ hg push
  pushing to ssh://localhost/$TESTTMP/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 2 files

  $ cd ../master
  $ hg log -l 1 --style compact
  3[tip]   1489bbbc46f0   1970-01-01 00:00 +0000   test
    a
  
  $ hg up
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat a
  a
