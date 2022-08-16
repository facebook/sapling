#chg-compatible
#debugruntest-compatible
  $ configure modernclient

Testing diff --change

  $ newclientrepo a

  $ echo "first" > file.txt
  $ hg add file.txt
  $ hg commit -m 'first commit' # 0
  $ hg push -q -r . --to head0 --create

  $ echo "second" > file.txt
  $ hg commit -m 'second commit' # 1

  $ echo "third" > file.txt
  $ hg commit -m 'third commit' # 2

  $ hg diff --nodates --change 'desc(second)'
  diff -r 4bb65dda5db4 -r e9b286083166 file.txt
  --- a/file.txt
  +++ b/file.txt
  @@ -1,1 +1,1 @@
  -first
  +second

  $ hg diff --change e9b286083166
  diff -r 4bb65dda5db4 -r e9b286083166 file.txt
  --- a/file.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -first
  +second
  $ hg push -q -r . --to book --create

Test dumb revspecs: top-level "x:y", "x:", ":y" and ":" ranges should be handled
as pairs even if x == y, but not for "f(x:y)" nor "x::y" (issue3474, issue4774)

  $ newclientrepo dumbspec test:a_server book
  $ echo "wdir" > file.txt

  $ hg diff -r 'desc(third)':'desc(third)'
  $ hg diff -r 'desc(third)':.
  $ hg diff -r 'desc(third)':
  $ hg diff -r :'desc(first)'
  $ hg diff -r 'desc(third):first(desc(third):desc(third))'
  $ hg diff -r 'first(desc(third):desc(third))' --nodates
  diff -r bf5ff72eb7e0 file.txt
  --- a/file.txt
  +++ b/file.txt
  @@ -1,1 +1,1 @@
  -third
  +wdir
  $ hg diff -r '(desc(third):desc(third))' --nodates
  diff -r bf5ff72eb7e0 file.txt
  --- a/file.txt
  +++ b/file.txt
  @@ -1,1 +1,1 @@
  -third
  +wdir
  $ hg diff -r 'desc(third)'::'desc(third)' --nodates
  diff -r bf5ff72eb7e0 file.txt
  --- a/file.txt
  +++ b/file.txt
  @@ -1,1 +1,1 @@
  -third
  +wdir
  $ hg diff -r "desc(third) and desc(second)"
  abort: empty revision range
  [255]

  $ newclientrepo dumbspec-rev0 test:a_server book head0
  $ hg up -q head0
  $ echo "wdir" > file.txt

  $ hg diff -r 'first(:)' --nodates
  diff -r 4bb65dda5db4 file.txt
  --- a/file.txt
  +++ b/file.txt
  @@ -1,1 +1,1 @@
  -first
  +wdir

  $ cd ..

Testing diff --change when merge:

  $ cd a

  $ for i in 1 2 3 4 5 6 7 8 9 10; do
  >    echo $i >> file.txt
  > done
  $ hg commit -m "lots of text" # 3

  $ sed -e 's,^2$,x,' file.txt > file.txt.tmp
  $ mv file.txt.tmp file.txt
  $ hg commit -m "change 2 to x" # 4

  $ hg up -r 'desc(lots)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ sed -e 's,^8$,y,' file.txt > file.txt.tmp
  $ mv file.txt.tmp file.txt
  $ hg commit -m "change 8 to y"

  $ hg up -C -r 273b50f17c6deb75b5a8652e5a9ca30bab9d8e40
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -r 'max(desc(change))'
  merging file.txt
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge 8 to y" # 6

  $ hg diff --change 'max(desc(change))'
  diff -r ae119d680c82 -r 9085c5c02e52 file.txt
  --- a/file.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -6,6 +6,6 @@
   5
   6
   7
  -8
  +y
   9
   10

must be similar to 'hg diff --change 5':

  $ hg diff -c 'desc(merge)'
  diff -r 273b50f17c6d -r 979ca961fd2e file.txt
  --- a/file.txt	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file.txt	Thu Jan 01 00:00:00 1970 +0000
  @@ -6,6 +6,6 @@
   5
   6
   7
  -8
  +y
   9
   10

  $ cd ..
