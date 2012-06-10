  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[mq]" >> $HGRCPATH
  $ echo "git=keep" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ echo 'base' > base
  $ hg ci -Ambase
  adding base

  $ hg qnew -mmqbase mqbase

  $ echo 'patched' > base
  $ hg qrefresh

qdiff:

  $ hg qdiff
  diff -r d20a80d4def3 base
  --- a/base	Thu Jan 01 00:00:00 1970 +0000
  +++ b/base* (glob)
  @@ -1,1 +1,1 @@
  -base
  +patched

qdiff dirname:

  $ hg qdiff --nodates .
  diff -r d20a80d4def3 base
  --- a/base
  +++ b/base
  @@ -1,1 +1,1 @@
  -base
  +patched

qdiff filename:

  $ hg qdiff --nodates base
  diff -r d20a80d4def3 base
  --- a/base
  +++ b/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg revert -a

  $ hg qpop
  popping mqbase
  patch queue now empty

  $ hg qdelete mqbase

  $ printf '1\n2\n3\n4\nhello world\ngoodbye world\n7\n8\n9\n' > lines
  $ hg ci -Amlines -d '2 0'
  adding lines

  $ hg qnew -mmqbase2 mqbase2
  $ printf '\n\n1\n2\n3\n4\nhello  world\n     goodbye world\n7\n8\n9\n' > lines

  $ hg qdiff --nodates -U 1
  diff -r b0c220e1cf43 lines
  --- a/lines
  +++ b/lines
  @@ -1,1 +1,3 @@
  +
  +
   1
  @@ -4,4 +6,4 @@
   4
  -hello world
  -goodbye world
  +hello  world
  +     goodbye world
   7

  $ hg qdiff --nodates -b
  diff -r b0c220e1cf43 lines
  --- a/lines
  +++ b/lines
  @@ -1,9 +1,11 @@
  +
  +
   1
   2
   3
   4
   hello world
  -goodbye world
  +     goodbye world
   7
   8
   9

  $ hg qdiff --nodates -U 1 -B
  diff -r b0c220e1cf43 lines
  --- a/lines
  +++ b/lines
  @@ -4,4 +4,4 @@
   4
  -hello world
  -goodbye world
  +hello  world
  +     goodbye world
   7

  $ hg qdiff --nodates -w
  diff -r b0c220e1cf43 lines
  --- a/lines
  +++ b/lines
  @@ -1,3 +1,5 @@
  +
  +
   1
   2
   3

  $ hg qdiff --nodates --reverse
  diff -r b0c220e1cf43 lines
  --- a/lines
  +++ b/lines
  @@ -1,11 +1,9 @@
  -
  -
   1
   2
   3
   4
  -hello  world
  -     goodbye world
  +hello world
  +goodbye world
   7
   8
   9

qdiff preserve existing git flag:

  $ hg qrefresh --git
  $ echo a >> lines
  $ hg qdiff
  diff --git a/lines b/lines
  --- a/lines
  +++ b/lines
  @@ -1,9 +1,12 @@
  +
  +
   1
   2
   3
   4
  -hello world
  -goodbye world
  +hello  world
  +     goodbye world
   7
   8
   9
  +a

  $ hg qdiff --stat
   lines |  7 +++++--
   1 files changed, 5 insertions(+), 2 deletions(-)
  $ hg qrefresh

qdiff when file deleted (but not removed) in working dir:

  $ hg qnew deleted-file
  $ echo a > newfile
  $ hg add newfile
  $ hg qrefresh
  $ rm newfile
  $ hg qdiff

  $ cd ..
