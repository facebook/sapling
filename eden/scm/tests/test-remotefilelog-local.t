
#require no-eden

#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ eagerepo
  $ hginit master
  $ cd master
  $ echo x > x
  $ echo y > y
  $ echo z > z
  $ hg commit -qAm xy
  $ hg book master

  $ cd ..

  $ newclientrepo shallow master

# status

  $ clearcache
  $ echo xx > x
  $ echo yy > y
  $ touch a
  $ hg status
  M x
  M y
  ? a
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s (?)
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
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob) (?)

# local commit

  $ clearcache
  $ echo a > a
  $ echo xxx > x
  $ echo yyy > y
  $ hg commit -m a
  ? files fetched over 1 fetches - (? misses, 0.00% hit ratio) over *s (glob) (?)

  $ hg -q up tip

# rebase

  $ clearcache
  $ cd ../master
  $ echo w > w
  $ hg commit -qAm w

  $ cd ../shallow
  $ hg pull
  pulling from test:master
  imported commit graph for 1 commit (1 segment)

  $ hg rebase -d master
  rebasing 9abfe7bca547 "a"
  3 files fetched over 2 fetches - (3 misses, 0.00% hit ratio) over *s (glob) (?)

# strip

  $ clearcache
  $ hg debugrebuilddirstate # fixes dirstate non-determinism
  $ hg debugstrip -r .
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  4 files fetched over 2 fetches - (4 misses, 0.00% hit ratio) over *s (glob) (?)

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

  $ hg up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  4 files fetched over 1 fetches - (4 misses, 0.00% hit ratio) over *s (glob) (?)
  $ cat a
  a

# revert
# The (re) below is an attempt to reduce some flakiness in what gets downloaded.
  $ clearcache
  $ hg revert -r .~2 y z
  no changes needed to z
  [12] files fetched over [12] fetches \- \([12] misses, 0.00% hit ratio\) over .*s (re) (?)
  $ hg checkout -C -r . -q

# explicit bundle should produce full bundle file

  $ hg bundle -r 'desc(a)' --base 'desc(w)' ../local.bundle
  2 changesets found
  $ cd ..

  $ newclientrepo shallow2 master
  $ hg unbundle ../local.bundle
  adding changesets
  adding manifests
  adding file changes

  $ hg log -r 'max(desc(a))' --stat
  commit:      19edf50f4de7
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
  $ hg merge 'desc(a)'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m merge
  $ hg debugstrip -q -r ".^"

# commit without producing new node

  $ cd $TESTTMP
  $ newclientrepo shallow3 master
  $ echo 1 > A
  $ hg commit -m foo -A A
  $ hg log -r . -T '{node}\n'
  383ce605500277f879b7460a16ba620eb6930b7f
  $ hg goto -r '.^' -q
  $ echo 1 > A
  $ hg commit -m foo -A A
  $ hg log -r . -T '{node}\n'
  383ce605500277f879b7460a16ba620eb6930b7f

test the file size limit by changing it to something really small
  $ echo "A moderately short sentence." > longfile
  $ hg add longfile
  $ hg ci -m longfile --config commit.file-size-limit=10
  abort: longfile: size of 29 bytes exceeds maximum size of 10 bytes!
  (use '--config commit.file-size-limit=N' to override)
  [255]
