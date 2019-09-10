  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create
  pushing rev 79c51fb96423 to destination ssh://user@dummy/master bookmark master
  searching for changes
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  remote: added 2 changesets with 2 changes to 2 files (?)
  exporting bookmark master
  $ cd ..

Shallow clone from full

  $ clone master shallow --noupdate
  $ cd shallow
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  lz4revlog
  remotefilelog
  revlogv1
  store
  treestate

  $ hg update
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06, found via 79c51fb96423
  2 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

Log on a file without -f

  $ hg log dir/y
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with -f

  $ hg log -f dir/y
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with kind in path
  $ hg log -r "filelog('path:dir/y')"
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on multiple files with -f

  $ hg log -f dir/y x
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
  changeset:   0:b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  
Log on a directory

  $ hg log dir
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file from inside a directory

  $ cd dir
  $ hg log y
  changeset:   1:79c51fb96423
  tag:         tip
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file via -fr
  $ cd ..
  $ hg log -fr tip dir/ --template '{rev}\n'
  1

Trace renames
  $ setconfig remotefilelog.localdatarepack=True
  $ echo >> x
  $ hg commit -m "Edit x"
  $ hg mv x z
  $ hg commit -m move
  $ hg repack
  $ hg log -f z -T '{desc}\n' -G --pager=off
  @  move
  |
  o  Edit x
  :
  o  x
  

Verify remotefilelog handles rename metadata stripping when comparing file sizes
  $ hg debugrebuilddirstate
  $ hg status
