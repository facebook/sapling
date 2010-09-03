Make status look into subrepositories by default:

  $ echo '[defaults]' >> $HGRCPATH
  $ echo 'status = -S' >> $HGRCPATH
  $ echo 'diff = --nodates -S' >> $HGRCPATH

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
  $ hg diff
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status call crossing repository boundaries:

  $ hg status foo/bar/z.txt
  M foo/bar/z.txt
  $ hg status -I 'foo/?.txt'
  M foo/y.txt
  $ hg status -I '**/?.txt'
  M foo/bar/z.txt
  M foo/y.txt
  $ hg diff -I '**/?.txt'
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status from within a subdirectory:

  $ mkdir dir
  $ cd dir
  $ echo a1 > a.txt
  $ hg status
  M foo/bar/z.txt
  M foo/y.txt
  ? dir/a.txt
  $ hg diff
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3

Status with relative path:

  $ hg status ..
  M ../foo/bar/z.txt
  M ../foo/y.txt
  ? a.txt
  $ hg diff ..
  diff -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,2 +1,3 @@
   y1
   y2
  +y3
  diff -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,2 +1,3 @@
   z1
   z2
  +z3
  $ cd ..

Cleanup and final commit:

  $ rm -r dir
  $ hg commit -m 2-3-2
  committing subrepository foo
  committing subrepository foo/bar

Log with the relationships between repo and its subrepo:

  $ hg log --template '{rev}:{node|short} {desc}\n'
  2:1326fa26d0c0 2-3-2
  1:4b3c9ff4f66b 1-2-1
  0:23376cbba0d8 0-0-0

  $ hg -R foo log --template '{rev}:{node|short} {desc}\n'
  3:65903cebad86 2-3-2
  2:d254738c5f5e 0-2-1
  1:8629ce7dcc39 0-1-0
  0:af048e97ade2 0-0-0

  $ hg -R foo/bar log --template '{rev}:{node|short} {desc}\n'
  2:31ecbdafd357 2-3-2
  1:9647f22de499 0-1-1
  0:4904098473f9 0-0-0

Status between revisions:

  $ hg status
  $ hg status --rev 0:1
  M .hgsubstate
  M foo/.hgsubstate
  M foo/bar/z.txt
  M foo/y.txt
  $ hg diff -I '**/?.txt' --rev 0:1
  diff -r af048e97ade2 -r d254738c5f5e foo/y.txt
  --- a/foo/y.txt
  +++ b/foo/y.txt
  @@ -1,1 +1,2 @@
   y1
  +y2
  diff -r 4904098473f9 -r 9647f22de499 foo/bar/z.txt
  --- a/foo/bar/z.txt
  +++ b/foo/bar/z.txt
  @@ -1,1 +1,2 @@
   z1
  +z2
