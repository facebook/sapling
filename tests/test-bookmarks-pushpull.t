  $ "$TESTDIR/hghave" serve || exit 80

  $ cat << EOF >> $HGRCPATH
  > [ui]
  > logtemplate={rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [extensions]
  > EOF
  $ cat > obs.py << EOF
  > import mercurial.obsolete
  > mercurial.obsolete._enabled = True
  > EOF
  $ echo "obs=${TESTTMP}/obs.py" >> $HGRCPATH

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
  adding remote bookmark X
  updating bookmark Y
  adding remote bookmark Z
  (run 'hg update' to get a working copy)
  $ hg bookmarks
     X                         0:4e3505fd9583
   * Y                         0:4e3505fd9583
     Z                         0:4e3505fd9583
  $ hg debugpushkey ../a namespaces
  bookmarks	
  namespaces	
  obsolete	
  phases	
  $ hg debugpushkey ../a bookmarks
  X	4e3505fd95835d721066b76e75dbb8cc554d7f77
  Y	4e3505fd95835d721066b76e75dbb8cc554d7f77
  Z	4e3505fd95835d721066b76e75dbb8cc554d7f77
  $ hg pull -B X ../a
  pulling from ../a
  no changes found
  importing bookmark X
  $ hg bookmark
     X                         0:4e3505fd9583
   * Y                         0:4e3505fd9583
     Z                         0:4e3505fd9583

export bookmark by name

  $ hg bookmark W
  $ hg bookmark foo
  $ hg bookmark foobar
  $ hg push -B W ../a
  pushing to ../a
  searching for changes
  no changes found
  exporting bookmark W
  [1]
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
  [1]

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
  $ hg book -f @
  $ hg book -f X
  $ hg book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         0:4e3505fd9583
     Z                         1:0d2164f0ce0d

  $ cd ../b
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark foobar
  $ echo c2 > f2
  $ hg ci -Am2
  adding f2
  $ hg book -if @
  $ hg book -if X
  $ hg book
     @                         1:9b140be10808
     X                         1:9b140be10808
     Y                         0:4e3505fd9583
     Z                         0:4e3505fd9583
     foo                       -1:000000000000
   * foobar                    1:9b140be10808

  $ hg pull --config paths.foo=../a foo
  pulling from $TESTTMP/a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  divergent bookmark @ stored as @foo
  divergent bookmark X stored as X@foo
  updating bookmark Z
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg book
     @                         1:9b140be10808
     @foo                      2:0d2164f0ce0d
     X                         1:9b140be10808
     X@foo                     2:0d2164f0ce0d
     Y                         0:4e3505fd9583
     Z                         2:0d2164f0ce0d
     foo                       -1:000000000000
   * foobar                    1:9b140be10808
  $ hg push -f ../a
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  $ hg -R ../a book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         0:4e3505fd9583
     Z                         1:0d2164f0ce0d

revsets should not ignore divergent bookmarks

  $ hg bookmark -fr 1 Z
  $ hg log -r 'bookmark()' --template '{rev}:{node|short} {bookmarks}\n'
  0:4e3505fd9583 Y
  1:9b140be10808 @ X Z foobar
  2:0d2164f0ce0d @foo X@foo
  $ hg log -r 'bookmark("X@foo")' --template '{rev}:{node|short} {bookmarks}\n'
  2:0d2164f0ce0d @foo X@foo
  $ hg log -r 'bookmark("re:X@foo")' --template '{rev}:{node|short} {bookmarks}\n'
  2:0d2164f0ce0d @foo X@foo

update a remote bookmark from a non-head to a head

  $ hg up -q Y
  $ echo c3 > f2
  $ hg ci -Am3
  adding f2
  created new head
  $ hg push ../a
  pushing to ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating bookmark Y
  $ hg -R ../a book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         3:f6fc62dde3c0
     Z                         1:0d2164f0ce0d

update a bookmark in the middle of a client pulling changes

  $ cd ..
  $ hg clone -q a pull-race
  $ hg clone -q pull-race pull-race2
  $ cd pull-race
  $ hg up -q Y
  $ echo c4 > f2
  $ hg ci -Am4
  $ echo c5 > f3
  $ cat <<EOF > .hg/hgrc
  > [hooks]
  > outgoing.makecommit = hg ci -Am5; echo committed in pull-race
  > EOF
  $ cd ../pull-race2
  $ hg pull
  pulling from $TESTTMP/pull-race (glob)
  searching for changes
  adding changesets
  adding f3
  committed in pull-race
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating bookmark Y
  (run 'hg update' to get a working copy)
  $ hg book
   * @                         1:0d2164f0ce0d
     X                         1:0d2164f0ce0d
     Y                         4:b0a5eff05604
     Z                         1:0d2164f0ce0d
  $ cd ../b

diverging a remote bookmark fails

  $ hg up -q 4e3505fd9583
  $ echo c4 > f2
  $ hg ci -Am4
  adding f2
  created new head
  $ echo c5 > f2
  $ hg ci -Am5
  $ hg log -G
  @  5:c922c0139ca0 5
  |
  o  4:4efff6d98829 4
  |
  | o  3:f6fc62dde3c0 3
  |/
  | o  2:0d2164f0ce0d 1
  |/
  | o  1:9b140be10808 2
  |/
  o  0:4e3505fd9583 test
  

  $ hg book -f Y

  $ cat <<EOF > ../a/.hg/hgrc
  > [web]
  > push_ssl = false
  > allow_push = *
  > EOF

  $ hg -R ../a serve -p $HGPORT2 -d --pid-file=../hg2.pid
  $ cat ../hg2.pid >> $DAEMON_PIDS

  $ hg push http://localhost:$HGPORT2/
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: push creates new remote head c922c0139ca0!
  (merge or see "hg help push" for details about pushing new heads)
  [255]
  $ hg -R ../a book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         3:f6fc62dde3c0
     Z                         1:0d2164f0ce0d


Unrelated marker does not alter the decision

  $ hg debugobsolete aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb
  $ hg push http://localhost:$HGPORT2/
  pushing to http://localhost:$HGPORT2/
  searching for changes
  abort: push creates new remote head c922c0139ca0!
  (merge or see "hg help push" for details about pushing new heads)
  [255]
  $ hg -R ../a book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         3:f6fc62dde3c0
     Z                         1:0d2164f0ce0d

Update to a successor works

  $ hg id --debug -r 3
  f6fc62dde3c0771e29704af56ba4d8af77abcc2f
  $ hg id --debug -r 4
  4efff6d98829d9c824c621afd6e3f01865f5439f
  $ hg id --debug -r 5
  c922c0139ca03858f655e4a2af4dd02796a63969 tip Y
  $ hg debugobsolete f6fc62dde3c0771e29704af56ba4d8af77abcc2f cccccccccccccccccccccccccccccccccccccccc
  $ hg debugobsolete cccccccccccccccccccccccccccccccccccccccc 4efff6d98829d9c824c621afd6e3f01865f5439f
  $ hg push http://localhost:$HGPORT2/
  pushing to http://localhost:$HGPORT2/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 2 changes to 1 files (+1 heads)
  updating bookmark Y
  $ hg -R ../a book
     @                         1:0d2164f0ce0d
   * X                         1:0d2164f0ce0d
     Y                         5:c922c0139ca0
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
  obsolete	
  phases	
  $ hg debugpushkey http://localhost:$HGPORT/ bookmarks
  @	9b140be1080824d768c5a4691a564088eede71f9
  X	9b140be1080824d768c5a4691a564088eede71f9
  Y	c922c0139ca03858f655e4a2af4dd02796a63969
  Z	9b140be1080824d768c5a4691a564088eede71f9
  foo	0000000000000000000000000000000000000000
  foobar	9b140be1080824d768c5a4691a564088eede71f9
  $ hg out -B http://localhost:$HGPORT/
  comparing with http://localhost:$HGPORT/
  searching for changed bookmarks
  no changed bookmarks found
  [1]
  $ hg push -B Z http://localhost:$HGPORT/
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  exporting bookmark Z
  [1]
  $ hg book -d Z
  $ hg in -B http://localhost:$HGPORT/
  comparing with http://localhost:$HGPORT/
  searching for changed bookmarks
     Z                         0d2164f0ce0d
     foo                       000000000000
     foobar                    9b140be10808
  $ hg pull -B Z http://localhost:$HGPORT/
  pulling from http://localhost:$HGPORT/
  no changes found
  divergent bookmark @ stored as @1
  divergent bookmark X stored as X@1
  adding remote bookmark Z
  adding remote bookmark foo
  adding remote bookmark foobar
  importing bookmark Z
  $ hg clone http://localhost:$HGPORT/ cloned-bookmarks
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 3 files (+2 heads)
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks bookmarks
   * @                         1:9b140be10808
     X                         1:9b140be10808
     Y                         4:c922c0139ca0
     Z                         2:0d2164f0ce0d
     foo                       -1:000000000000
     foobar                    1:9b140be10808

  $ cd ..

Pushing a bookmark should only push the changes required by that
bookmark, not all outgoing changes:
  $ hg clone http://localhost:$HGPORT/ addmarks
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 3 files (+2 heads)
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd addmarks
  $ echo foo > foo
  $ hg add foo
  $ hg commit -m 'add foo'
  $ echo bar > bar
  $ hg add bar
  $ hg commit -m 'add bar'
  $ hg co "tip^"
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg book add-foo
  $ hg book -r tip add-bar
Note: this push *must* push only a single changeset, as that's the point
of this test.
  $ hg push -B add-foo --traceback
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  exporting bookmark add-foo

  $ cd ..
