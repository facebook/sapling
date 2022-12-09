#chg-compatible
#require mononoke
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create --config paths.default=mononoke://$(mononoke_address)/master
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  pushing rev 79c51fb96423 to destination mononoke://$LOCALIP:$LOCAL_PORT/master bookmark master
  searching for changes
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

  $ hg goto
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06
  1 trees fetched over 0.00s
  fetching tree 'dir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
  1 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Log on a file without -f

  $ hg log dir/y
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with -f

  $ hg log -f dir/y
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file with kind in path
  $ hg log -r "filelog('path:dir/y')"
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on multiple files with -f

  $ hg log -f dir/y x
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
  commit:      b292c1e3311f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     x
  
Log on a directory

  $ hg log dir
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file from inside a directory

  $ cd dir
  $ hg log y
  commit:      79c51fb96423
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     y
  
Log on a file via -fr
  $ cd ..
  $ hg log -fr tip dir/ --template '{node}\n'
  79c51fb9642383579314de1dcd88e4dd7b1b518a

Trace renames
  $ echo >> x
  $ hg commit -m "Edit x"
  $ hg mv x z
  $ hg commit -m move
  $ hg log -f z -T '{desc}\n' -G --pager=off
  @  move
  │
  o  Edit x
  ╷
  o  x
  

Verify remotefilelog handles rename metadata stripping when comparing file sizes
  $ hg debugrebuilddirstate
  $ hg status
