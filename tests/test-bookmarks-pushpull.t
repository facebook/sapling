  $ echo "[extensions]" >> $HGRCPATH
  $ echo "bookmarks=" >> $HGRCPATH

  $ echo "[bookmarks]" >> $HGRCPATH
  $ echo "track.current = True" >> $HGRCPATH

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
  $ hg pull ../a
  pulling from ../a
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg bookmarks
  no bookmarks set
  $ hg pull -B X ../a
  pulling from ../a
  searching for changes
  no changes found
  importing bookmark X
  $ hg bookmark
     X                         0:4e3505fd9583

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
     Y                         0:4e3505fd9583
     X                         0:4e3505fd9583
   * Z                         0:4e3505fd9583
     W                         -1:000000000000

push/pull name that doesn't exist

  $ hg push -B badname ../a
  bookmark badname does not exist on the local or remote repository!
  $ hg pull -B anotherbadname ../a
  abort: remote bookmark anotherbadname not found!
  $ true
