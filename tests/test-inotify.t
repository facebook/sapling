
  $ "$TESTDIR/hghave" inotify || exit 80
  $ hg init repo1
  $ cd repo1
  $ touch a b c d e
  $ mkdir dir
  $ mkdir dir/bar
  $ touch dir/x dir/y dir/bar/foo
  $ hg ci -Am m
  adding a
  adding b
  adding c
  adding d
  adding dir/bar/foo
  adding dir/x
  adding dir/y
  adding e
  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "inotify=" >> $HGRCPATH
  $ cd repo2
  $ echo b >> a

check that daemon started automatically works correctly
and make sure that inotify.pidfile works

  $ hg --config "inotify.pidfile=../hg2.pid" status
  M a

make sure that pidfile worked. Output should be silent.

  $ kill `cat ../hg2.pid`
  $ cd ../repo1

inserve

  $ hg inserve -d --pid-file=hg.pid
  $ cat hg.pid >> "$DAEMON_PIDS"

let the daemon finish its stuff

  $ sleep 1

cannot start, already bound

  $ hg inserve
  abort: inotify-server: cannot start: socket is already bound
  [255]

issue907

  $ hg status
  ? hg.pid

clean

  $ hg status -c
  C a
  C b
  C c
  C d
  C dir/bar/foo
  C dir/x
  C dir/y
  C e

all

  $ hg status -A
  ? hg.pid
  C a
  C b
  C c
  C d
  C dir/bar/foo
  C dir/x
  C dir/y
  C e

path patterns

  $ echo x > dir/x
  $ hg status .
  M dir/x
  ? hg.pid
  $ hg status dir
  M dir/x
  $ cd dir
  $ hg status .
  M x
  $ cd ..

issue 1375
testing that we can remove a folder and then add a file with the same name
issue 1375

  $ mkdir h
  $ echo h > h/h
  $ hg ci -Am t
  adding h/h
  adding hg.pid
  $ hg rm h
  removing h/h
  $ echo h >h
  $ hg add h
  $ hg status
  A h
  R h/h
  $ hg ci -m0

Test for issue1735: inotify watches files in .hg/merge

  $ hg st
  $ echo a > a
  $ hg ci -Am a
  $ hg st
  $ echo b >> a
  $ hg ci -m ab
  $ hg st
  $ echo c >> a
  $ hg st
  M a
  $ HGMERGE=internal:local hg up 0
  1 files updated, 1 files merged, 2 files removed, 0 files unresolved
  $ hg st
  M a
  $ HGMERGE=internal:local hg up
  3 files updated, 1 files merged, 0 files removed, 0 files unresolved
  $ hg st
  M a

Test for 1844: "hg ci folder" will not commit all changes beneath "folder"

  $ mkdir 1844
  $ echo a > 1844/foo
  $ hg add 1844
  adding 1844/foo
  $ hg ci -m 'working'
  $ echo b >> 1844/foo
  $ hg ci 1844 -m 'broken'

Test for issue884: "Build products not ignored until .hgignore is touched"

  $ echo '^build$' > .hgignore
  $ hg add .hgignore
  $ hg ci .hgignore -m 'ignorelist'

Now, lets add some build products...

  $ mkdir build
  $ touch build/x
  $ touch build/y

build/x & build/y shouldn't appear in "hg st"

  $ hg st
  $ kill `cat hg.pid`
