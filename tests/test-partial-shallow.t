  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > foo
  $ echo y > bar
  $ hg commit -qAm one

  $ cd ..

# partial shallow clone

  $ hg clone --shallow ssh://localhost/$PWD/master shallow --noupdate --config remotefilelog.includepattern=foo
  streaming all changes
  3 files to transfer, 336 bytes of data
  transferred 336 bytes in * seconds (* KB/sec) (glob)
  $ cat >> shallow/.hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$PWD/hgcache
  > debug=True
  > includepattern=foo
  > [extensions]
  > remotefilelog=$TESTDIR/../remotefilelog
  > EOF
  $ ls shallow/.hg/store/data
  bar.i

# update partial clone

  $ cd shallow
  $ hg update
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cat foo
  x
  $ cat bar
  y
  $ cd ..

# pull partial clone

  $ cd master
  $ echo a >> foo
  $ echo b >> bar
  $ hg commit -qm two
  $ cd ../shallow
  $ hg pull
  pulling from ssh://localhost/$TESTTMP/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)
  $ hg update
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cat foo
  x
  a
  $ cat bar
  y
  b

  $ cd ..
