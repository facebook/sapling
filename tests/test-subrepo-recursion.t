Make status look into subrepositories by default:

  $ echo '[defaults]' >> $HGRCPATH
  $ echo 'status = -S' >> $HGRCPATH

Create test repository:

  $ hg init
  $ echo x1 > x.txt
  $ hg add x.txt

  $ hg init foo
  $ cd foo
  $ echo y1 > y.txt
  $ hg add y.txt

  $ hg init bar
  $ cd bar
  $ echo z1 > z.txt
  $ hg add z.txt

  $ cd ..
  $ echo 'bar = bar' > .hgsub
  $ hg add .hgsub

  $ cd ..
  $ echo 'foo = foo' > .hgsub
  $ hg add .hgsub

  $ hg commit -m 0-0-0
  committing subrepository foo
  committing subrepository foo/bar

  $ cd foo
  $ echo y2 >> y.txt
  $ hg commit -m 0-1-0

  $ cd bar
  $ echo z2 >> z.txt
  $ hg commit -m 0-1-1

  $ cd ..
  $ hg commit -m 0-2-1
  committing subrepository bar

  $ cd ..
  $ hg commit -m 1-2-1
  committing subrepository foo

Change working directory:

  $ echo y3 >> foo/y.txt
  $ echo z3 >> foo/bar/z.txt
  $ hg status
  M foo/bar/z.txt
  M foo/y.txt

Status call crossing repository boundaries:

  $ hg status foo/bar/z.txt
  M foo/bar/z.txt
  $ hg status -I 'foo/?.txt'
  M foo/y.txt
  $ hg status -I '**/?.txt'
  M foo/bar/z.txt
  M foo/y.txt

Status from within a subdirectory:

  $ mkdir dir
  $ cd dir
  $ echo a1 > a.txt
  $ hg status
  M foo/bar/z.txt
  M foo/y.txt
  ? dir/a.txt

Status with relative path:

  $ hg status ..
  M ../foo/bar/z.txt
  M ../foo/y.txt
  ? a.txt
  $ cd ..

Status between revisions:

  $ rm -r dir
  $ hg commit -m 2-2-1
  committing subrepository foo
  committing subrepository foo/bar
  $ hg status
  $ hg status --rev 0:1
  M .hgsubstate
  M foo/.hgsubstate
  M foo/bar/z.txt
  M foo/y.txt
