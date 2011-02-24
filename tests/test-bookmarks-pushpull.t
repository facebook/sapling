initialize

  $ hg init a
  $ cd a
  $ echo 'test' > test
  $ hg commit -Am'test'
  adding test

set bookmarks

  $ hg bookmark X
  $ hg bookmark Y
  $ hg bookmark Z

import bookmark by name

  $ hg init ../b
  $ cd ../b
  $ hg book Y
  $ hg book
   * Y                         -1:000000000000
  $ hg pull ../a
  pulling from ../a
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg bookmarks
     Y                         0:4e3505fd9583
  $ hg debugpushkey ../a namespaces
  bookmarks	
  namespaces	
  $ hg debugpushkey ../a bookmarks
  Y	4e3505fd95835d721066b76e75dbb8cc554d7f77
  X	4e3505fd95835d721066b76e75dbb8cc554d7f77
  Z	4e3505fd95835d721066b76e75dbb8cc554d7f77
  $ hg pull -B X ../a
  pulling from ../a
  searching for changes
  no changes found
  importing bookmark X
  $ hg bookmark
     X                         0:4e3505fd9583
     Y                         0:4e3505fd9583

export bookmark by name

  $ hg bookmark W
  $ hg bookmark foo
  $ hg bookmark foobar
  $ hg push -B W ../a
  pushing to ../a
  searching for changes
  no changes found
  exporting bookmark W
  $ hg -R ../a bookmarks
     W                         -1:000000000000
     X                         0:4e3505fd9583
     Y                         0:4e3505fd9583
   * Z                         0:4e3505fd9583

delete a remote bookmark

  $ hg book -d W
  $ hg push -B W ../a
  pushing to ../a
  searching for changes
  no changes found
  deleting remote bookmark W

push/pull name that doesn't exist

  $ hg push -B badname ../a
  pushing to ../a
  searching for changes
  no changes found
  bookmark badname does not exist on the local or remote repository!
  [2]
  $ hg pull -B anotherbadname ../a
  pulling from ../a
  abort: remote bookmark anotherbadname not found!
  [255]

divergent bookmarks

  $ cd ../a
  $ echo c1 > f1
  $ hg ci -Am1
  adding f1
  $ hg book -f X
  $ hg book
   * X                         1:0d2164f0ce0d
     Y                         0:4e3505fd9583
     Z                         1:0d2164f0ce0d

  $ cd ../b
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c2 > f2
  $ hg ci -Am2
  adding f2
  $ hg book -f X
  $ hg book
   * X                         1:9b140be10808
     Y                         0:4e3505fd9583
     foo                       -1:000000000000
     foobar                    -1:000000000000

  $ hg pull ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  not updating divergent bookmark X
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg book
   * X                         1:9b140be10808
     Y                         0:4e3505fd9583
     foo                       -1:000000000000
     foobar                    -1:000000000000
  $ hg push -f ../a
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  $ hg -R ../a book
   * X                         1:0d2164f0ce0d
     Y                         0:4e3505fd9583
     Z                         1:0d2164f0ce0d

hgweb

  $ cat <<EOF > .hg/hgrc
  > [web]
  > push_ssl = false
  > allow_push = *
  > EOF

  $ hg serve -p $HGPORT -d --pid-file=../hg.pid -E errors.log
  $ cat ../hg.pid >> $DAEMON_PIDS
  $ cd ../a

  $ hg debugpushkey http://localhost:$HGPORT/ namespaces 
  bookmarks	
  namespaces	
  $ hg debugpushkey http://localhost:$HGPORT/ bookmarks
  Y	4e3505fd95835d721066b76e75dbb8cc554d7f77
  X	9b140be1080824d768c5a4691a564088eede71f9
  foo	0000000000000000000000000000000000000000
  foobar	0000000000000000000000000000000000000000
  $ hg out -B http://localhost:$HGPORT/
  comparing with http://localhost:$HGPORT/
  searching for changed bookmarks
     Z                         0d2164f0ce0d
  $ hg push -B Z http://localhost:$HGPORT/
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  exporting bookmark Z
  $ hg book -d Z
  $ hg in -B http://localhost:$HGPORT/
  comparing with http://localhost:$HGPORT/
  searching for changed bookmarks
     Z                         0d2164f0ce0d
     foo                       000000000000
     foobar                    000000000000
  $ hg pull -B Z http://localhost:$HGPORT/
  pulling from http://localhost:$HGPORT/
  searching for changes
  no changes found
  not updating divergent bookmark X
  importing bookmark Z

  $ kill `cat ../hg.pid`
