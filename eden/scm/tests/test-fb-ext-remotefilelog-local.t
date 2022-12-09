#chg-compatible
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest

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
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over *s (glob) (?)
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

# local commit where the dirstate is clean -- ensure that we do just one fetch
# (update to a commit on the server first)

  $ hg --config debug.dirstate.delaywrite=1 up 'desc(xy)'
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ clearcache
  $ hg debugdirstate
  n 644          2 * x (glob)
  n 644          2 * y (glob)
  n 644          2 * z (glob)
  $ echo xxxx > x
  $ echo yyyy > y
  $ hg commit -m x
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)

# restore state for future tests

  $ hg -q debugstrip .
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

  $ hg rebase -d tip
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

  $ hg up
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

  $ hgcloneshallow ssh://user@dummy/master shallow2 -q
  [12] files fetched over 1 fetches \- \([12] misses, 0.00% hit ratio\) over .*s (re) (?)
  $ cd shallow2
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
  $ hgcloneshallow ssh://user@dummy/master shallow3 -q
  $ cd shallow3
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
  $ cat > ../sizelimit.py <<EOF
  > from __future__ import absolute_import
  > import edenscm.ext.remotefilelog.remotefilelog as remotefilelog
  > 
  > def uisetup(ui):
  >     remotefilelog._maxentrysize = ui.configint('sizelimit', 'sizelimit')
  > EOF
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > sizelimit = `pwd`/../sizelimit.py
  > [sizelimit]
  > sizelimit = 10
  > EOF

  $ echo "A moderately short sentence." > longfile
  $ hg add longfile
  $ hg ci -m longfile
  abort: longfile: size of 29 bytes exceeds maximum size of 10 bytes!
  [255]
