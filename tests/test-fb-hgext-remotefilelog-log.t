  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ cd master
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y

  $ cd ..

Shallow clone from full

  $ clone master shallow --noupdate
  streaming all changes
  2 files to transfer, 472 bytes of data
  transferred 472 bytes in * seconds (*/sec) (glob)
  searching for changes
  no changes found
  $ cd shallow
  $ cat .hg/requires
  dotencode
  fncache
  generaldelta
  remotefilelog
  revlogv1
  store
  treestate

  $ hg update
  fetching tree '' 479230b8a7bab24c6717f4997ec84092d304b5dd, found via 2e73264fab97
  2 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

Log on a file without -f

  $ hg log dir/y
  warning: file log can be slow on large repos - use -f to speed it up
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with -f

  $ hg log -f dir/y
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with kind in path
  $ hg log -r "filelog('path:dir/y')"
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on multiple files with -f

  $ hg log -f dir/y x
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
  changeset:   0:b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  
Log on a directory

  $ hg log dir
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file from inside a directory

  $ cd dir
  $ hg log y
  warning: file log can be slow on large repos - use -f to speed it up
  changeset:   1:2e73264fab97
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file via -fr
  $ cd ..
  $ hg log -fr tip dir/ --template '{rev}\n'
  1

Trace renames
- Enable local packs and rust history packs to test a bug involving tracking
- renames across packs.
  $ setconfig remotefilelog.packlocaldata=True remotefilelog.localdatarepack=True
  $ setconfig format.userusthistorypack=True
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
