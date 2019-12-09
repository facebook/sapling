#chg-compatible

  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > foo
  $ echo y > bar
  $ hg commit -qAm one
  $ hg tag tag1
  $ cd ..

# clone with tags

  $ hg clone --shallow ssh://user@dummy/master shallow --noupdate --config remotefilelog.excludepattern=.hgtags
  streaming all changes
  4 files to transfer, * of data (glob)
  transferred * bytes in * (*) (glob)
  searching for changes
  no changes found
  $ cat >> shallow/.hg/hgrc <<EOF
  > [remotefilelog]
  > cachepath=$PWD/hgcache
  > debug=True
  > reponame = master
  > excludepattern=.hgtags
  > [extensions]
  > remotefilelog=
  > EOF

  $ cd shallow
  $ ls .hg/store/data
  ~2ehgtags.i
  $ hg tags
  tip                                1:6ce44dcfda68
  tag1                               0:e0360bc0d9e1
  $ hg update
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

# pull with tags

  $ cd ../master
  $ hg tag tag2
  $ cd ../shallow
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 6a22dfa4fd34
  $ hg tags
  tip                                2:6a22dfa4fd34
  tag2                               1:6ce44dcfda68
  tag1                               0:e0360bc0d9e1
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls .hg/store/data
  ~2ehgtags.i

  $ hg log -l 1 --stat
  changeset:   2:6a22dfa4fd34
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag tag2 for changeset 6ce44dcfda68
  
   .hgtags |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
