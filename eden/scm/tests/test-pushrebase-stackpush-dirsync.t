#require no-eden

  $ enable pushrebase dirsync

The pushback of mutation markers through a local peer is unrelated to
dirsync and fails on its own, so turn it off.

  $ setconfig pushrebase.pushback.obsmarkers=false

Server with a committed .hgdirsync config:

  $ newrepo server
  $ cat > .hgdirsync <<EOF
  > sync1.dir1 = dir1/
  > sync1.dir2 = dir2/
  > EOF
  $ mkdir dir1
  $ echo 1 > dir1/a
  $ sl commit -Am base
  adding .hgdirsync
  adding dir1/a
  mirrored adding 'dir1/a' to 'dir2/a'
  $ sl bookmark master

  $ cd $TESTTMP
  $ sl clone server client
  updating to tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

A commit on the server, so the push has to rebase:

  $ cd $TESTTMP/server
  $ echo 2 > unrelated
  $ sl commit -Am s1
  adding unrelated
  $ sl bookmark master

A stack on the client that touches a synced directory but not .hgdirsync.
dirsync is off on the client, so the server has to mirror during pushrebase:

  $ cd $TESTTMP/client
  $ echo 3 > dir1/b
  $ sl commit -Am c1 --config extensions.dirsync=!
  adding dir1/b
  $ echo 4 > dir1/c
  $ sl commit -Am c2 --config extensions.dirsync=!
  adding dir1/c

  $ sl push --to master 2>&1 | grep -E '^(searching for changes|AssertionError|mirrored|updating bookmark)'
  searching for changes
  AssertionError

  $ cd $TESTTMP/server
  $ sl log -G -T '{desc} {files} {bookmarks}\n' -r 'all()'
  @  s1 unrelated master
  │
  o  base .hgdirsync dir1/a dir2/a
  $ sl cat -r master dir2/c
  dir2/c: no such file in rev 7f5f99663f02
  [1]
