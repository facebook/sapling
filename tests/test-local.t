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

  $ hg debugrebuilddirstate # fixes dirstate non-determinism
  $ hg add a
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
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

# local commit

  $ clearcache
  $ echo a > a
  $ echo xxx > x
  $ echo yyy > y
  $ hg commit -m a
  ? files fetched over 1 fetches - (? misses, 0.00% hit ratio) over *s (glob)

# local commit where the dirstate is clean -- ensure that we do just one fetch
# (update to a commit on the server first)

  $ hg --config debug.dirstate.delaywrite=1 up 0
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ clearcache
  $ hg debugdirstate
  n 644          2 * x (glob)
  n 644          2 * y (glob)
  n 644          2 * z (glob)
  $ echo xxxx > x
  $ echo yyyy > y
  $ hg commit -m x
  created new head
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

# restore state for future tests

  $ hg -q strip .
  $ hg -q up tip

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
  rebasing 1:9abfe7bca547 "a"
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/9abfe7bca547-8b11e5ff-backup.hg (glob)
  3 files fetched over 2 fetches - (3 misses, 0.00% hit ratio) over *s (glob)

# strip

  $ clearcache
  $ hg debugrebuilddirstate # fixes dirstate non-determinism
  $ hg strip -r .
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/shallow/.hg/strip-backup/19edf50f4de7-df3d0f74-backup.hg (glob)
  4 files fetched over 2 fetches - (4 misses, 0.00% hit ratio) over *s (glob)

# unbundle

  $ clearcache
  $ ls
  w
  x
  y
  z

  $ hg debugrebuilddirstate # fixes dirstate non-determinism
  $ hg unbundle .hg/strip-backup/19edf50f4de7-df3d0f74-backup.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  (run 'hg update' to get a working copy)

  $ hg up
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over *s (glob)
  $ cat a
  a

# revert

  $ clearcache
  $ hg revert -r .~2 y z
  no changes needed to z
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ hg checkout -C -r . -q

# explicit bundle should produce full bundle file

  $ hg bundle -r 2 --base 1 ../local.bundle
  1 changesets found
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over *s (glob)
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
  
# Merge

  $ echo merge >> w
  $ hg commit -m w
  created new head
  $ hg merge 2
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m merge
  $ hg strip -q -r .^
