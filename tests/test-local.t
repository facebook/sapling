  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ hg commit -qAm xy

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ cd shallow

# status

  $ clearcache
  $ echo xx > x
  $ echo yy > y
  $ touch a
  $ hg status
  M x
  M y
  ? a
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ hg add a
  $ hg status
  M x
  M y
  A a

# diff

  $ clearcache
  $ hg diff
  diff -r f3d0bb0d1e48 x
  --- a/x* (glob)
  +++ b/x* (glob)
  @@ -1,1 +1,1 @@
  -x
  +xx
  diff -r f3d0bb0d1e48 y
  --- a/y* (glob)
  +++ b/y* (glob)
  @@ -1,1 +1,1 @@
  -y
  +yy
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

# local commit

  $ clearcache
  $ echo a > a
  $ echo xxx > x
  $ echo yyy > y
  $ hg commit -m a
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

# rebase

  $ clearcache
  $ cd ../master
  $ echo w > w
  $ hg commit -qAm w

  $ cd ../shallow
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg rebase -d tip
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/9abfe7bca547-backup.hg
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

# strip

  $ clearcache
  $ hg strip -r .
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/19edf50f4de7-backup.hg
  3 files fetched over 2 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

# unbundle

  $ clearcache
  $ ls
  w
  x
  y
  z

  $ hg unbundle .hg/strip-backup/19edf50f4de7-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)

  $ hg up
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ cat a
  a

# revert

  $ clearcache
  $ hg revert -r .~2 y z
  3 files fetched over 2 fetches - (3 misses, 0.00% hit ratio) over *s (glob)
  $ hg checkout -C -r . -q

# explicit bundle should produce full bundle file

  $ hg bundle -r 2 --base 1 ../local.bundle
  1 changesets found
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.10s
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  $ cd shallow2
  $ hg unbundle ../local.bundle
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  (run 'hg update' to get a working copy)

  $ hg log -r 2 --stat
  changeset:   2:19edf50f4de7
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
   a |  1 +
   x |  2 +-
   y |  2 +-
   3 files changed, 3 insertions(+), 2 deletions(-)
  
