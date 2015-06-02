#require serve

  $ cat << EOF >> $HGRCPATH
  > [ui]
  > logtemplate={rev}:{node|short} {desc|firstline}
  > [phases]
  > publish=False
  > [experimental]
  > evolution=createmarkers,exchange
  > # drop me once bundle2 is the default,
  > # added to get test change early.
  > bundle2-exp = True
  > EOF

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

delete the bookmark to re-pull it

  $ hg book -d X
  $ hg pull -B X ../a
  pulling from ../a
  no changes found
  adding remote bookmark X

finally no-op pull

  $ hg pull -B X ../a
  pulling from ../a
  no changes found
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
  bookmark badname does not exist on the local or remote repository!
  no changes found
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

(test that too many divergence of bookmark)

  $ python $TESTDIR/seq.py 1 100 | while read i; do hg bookmarks -r 000000000000 "X@${i}"; done
  $ hg pull ../a
  pulling from ../a
  searching for changes
  no changes found
  warning: failed to assign numbered name to divergent bookmark X
  divergent bookmark @ stored as @1
  $ hg bookmarks | grep '^   X' | grep -v ':000000000000'
     X                         1:9b140be10808
     X@foo                     2:0d2164f0ce0d

(test that remotely diverged bookmarks are reused if they aren't changed)

  $ hg bookmarks | grep '^   @'
     @                         1:9b140be10808
     @1                        2:0d2164f0ce0d
     @foo                      2:0d2164f0ce0d
  $ hg pull ../a
  pulling from ../a
  searching for changes
  no changes found
  warning: failed to assign numbered name to divergent bookmark X
  divergent bookmark @ stored as @1
  $ hg bookmarks | grep '^   @'
     @                         1:9b140be10808
     @1                        2:0d2164f0ce0d
     @foo                      2:0d2164f0ce0d

  $ python $TESTDIR/seq.py 1 100 | while read i; do hg bookmarks -d "X@${i}"; done
  $ hg bookmarks -d "@1"

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

explicit pull should overwrite the local version (issue4439)

  $ hg pull --config paths.foo=../a foo -B X
  pulling from $TESTTMP/a (glob)
  no changes found
  divergent bookmark @ stored as @foo
  importing bookmark X

reinstall state for further testing:

  $ hg book -fr 9b140be10808 X

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
  $ hg -R $TESTTMP/pull-race book
     @                         1:0d2164f0ce0d
     X                         1:0d2164f0ce0d
   * Y                         4:b0a5eff05604
     Z                         1:0d2164f0ce0d
  $ hg pull
  pulling from $TESTTMP/pull-race (glob)
  searching for changes
  adding f3
  committed in pull-race
  adding changesets
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
  abort: push creates new remote head c922c0139ca0 with bookmark 'Y'!
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
  abort: push creates new remote head c922c0139ca0 with bookmark 'Y'!
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
  remote: 2 new obsolescence markers
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
     @                         0d2164f0ce0d
     X                         0d2164f0ce0d
     Z                         0d2164f0ce0d
     foo                                   
     foobar                                
  $ hg push -B Z http://localhost:$HGPORT/
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  updating bookmark Z
  [1]
  $ hg book -d Z
  $ hg in -B http://localhost:$HGPORT/
  comparing with http://localhost:$HGPORT/
  searching for changed bookmarks
     @                         9b140be10808
     X                         9b140be10808
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
  $ hg clone http://localhost:$HGPORT/ cloned-bookmarks
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 3 files (+2 heads)
  2 new obsolescence markers
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

Test to show result of bookmarks comparision

  $ mkdir bmcomparison
  $ cd bmcomparison

  $ hg init source
  $ hg -R source debugbuilddag '+2*2*3*4'
  $ hg -R source log -G --template '{rev}:{node|short}'
  o  4:e7bd5218ca15
  |
  | o  3:6100d3090acf
  |/
  | o  2:fa942426a6fd
  |/
  | o  1:66f7d451a68b
  |/
  o  0:1ea73414a91b
  
  $ hg -R source bookmarks -r 0 SAME
  $ hg -R source bookmarks -r 0 ADV_ON_REPO1
  $ hg -R source bookmarks -r 0 ADV_ON_REPO2
  $ hg -R source bookmarks -r 0 DIFF_ADV_ON_REPO1
  $ hg -R source bookmarks -r 0 DIFF_ADV_ON_REPO2
  $ hg -R source bookmarks -r 1 DIVERGED

  $ hg clone -U source repo1

(test that incoming/outgoing exit with 1, if there is no bookmark to
be excahnged)

  $ hg -R repo1 incoming -B
  comparing with $TESTTMP/bmcomparison/source
  searching for changed bookmarks
  no changed bookmarks found
  [1]
  $ hg -R repo1 outgoing -B
  comparing with $TESTTMP/bmcomparison/source
  searching for changed bookmarks
  no changed bookmarks found
  [1]

  $ hg -R repo1 bookmarks -f -r 1 ADD_ON_REPO1
  $ hg -R repo1 bookmarks -f -r 2 ADV_ON_REPO1
  $ hg -R repo1 bookmarks -f -r 3 DIFF_ADV_ON_REPO1
  $ hg -R repo1 bookmarks -f -r 3 DIFF_DIVERGED
  $ hg -R repo1 -q --config extensions.mq= strip 4
  $ hg -R repo1 log -G --template '{node|short} ({bookmarks})'
  o  6100d3090acf (DIFF_ADV_ON_REPO1 DIFF_DIVERGED)
  |
  | o  fa942426a6fd (ADV_ON_REPO1)
  |/
  | o  66f7d451a68b (ADD_ON_REPO1 DIVERGED)
  |/
  o  1ea73414a91b (ADV_ON_REPO2 DIFF_ADV_ON_REPO2 SAME)
  

  $ hg clone -U source repo2
  $ hg -R repo2 bookmarks -f -r 1 ADD_ON_REPO2
  $ hg -R repo2 bookmarks -f -r 1 ADV_ON_REPO2
  $ hg -R repo2 bookmarks -f -r 2 DIVERGED
  $ hg -R repo2 bookmarks -f -r 4 DIFF_ADV_ON_REPO2
  $ hg -R repo2 bookmarks -f -r 4 DIFF_DIVERGED
  $ hg -R repo2 -q --config extensions.mq= strip 3
  $ hg -R repo2 log -G --template '{node|short} ({bookmarks})'
  o  e7bd5218ca15 (DIFF_ADV_ON_REPO2 DIFF_DIVERGED)
  |
  | o  fa942426a6fd (DIVERGED)
  |/
  | o  66f7d451a68b (ADD_ON_REPO2 ADV_ON_REPO2)
  |/
  o  1ea73414a91b (ADV_ON_REPO1 DIFF_ADV_ON_REPO1 SAME)
  

(test that difference of bookmarks between repositories are fully shown)

  $ hg -R repo1 incoming -B repo2 -v
  comparing with repo2
  searching for changed bookmarks
     ADD_ON_REPO2              66f7d451a68b added
     ADV_ON_REPO2              66f7d451a68b advanced
     DIFF_ADV_ON_REPO2         e7bd5218ca15 changed
     DIFF_DIVERGED             e7bd5218ca15 changed
     DIVERGED                  fa942426a6fd diverged
  $ hg -R repo1 outgoing -B repo2 -v
  comparing with repo2
  searching for changed bookmarks
     ADD_ON_REPO1              66f7d451a68b added
     ADD_ON_REPO2                           deleted
     ADV_ON_REPO1              fa942426a6fd advanced
     DIFF_ADV_ON_REPO1         6100d3090acf advanced
     DIFF_ADV_ON_REPO2         1ea73414a91b changed
     DIFF_DIVERGED             6100d3090acf changed
     DIVERGED                  66f7d451a68b diverged

  $ hg -R repo2 incoming -B repo1 -v
  comparing with repo1
  searching for changed bookmarks
     ADD_ON_REPO1              66f7d451a68b added
     ADV_ON_REPO1              fa942426a6fd advanced
     DIFF_ADV_ON_REPO1         6100d3090acf changed
     DIFF_DIVERGED             6100d3090acf changed
     DIVERGED                  66f7d451a68b diverged
  $ hg -R repo2 outgoing -B repo1 -v
  comparing with repo1
  searching for changed bookmarks
     ADD_ON_REPO1                           deleted
     ADD_ON_REPO2              66f7d451a68b added
     ADV_ON_REPO2              66f7d451a68b advanced
     DIFF_ADV_ON_REPO1         1ea73414a91b changed
     DIFF_ADV_ON_REPO2         e7bd5218ca15 advanced
     DIFF_DIVERGED             e7bd5218ca15 changed
     DIVERGED                  fa942426a6fd diverged

  $ cd ..

Pushing a bookmark should only push the changes required by that
bookmark, not all outgoing changes:
  $ hg clone http://localhost:$HGPORT/ addmarks
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 3 files (+2 heads)
  2 new obsolescence markers
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
  (leaving bookmark @)
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

pushing a new bookmark on a new head does not require -f if -B is specified

  $ hg up -q X
  $ hg book W
  $ echo c5 > f2
  $ hg ci -Am5
  created new head
  $ hg push -B W
  pushing to http://localhost:$HGPORT/
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)
  exporting bookmark W
  $ hg -R ../b id -r W
  cc978a373a53 tip W

Check summary output for incoming/outgoing bookmarks

  $ hg bookmarks -d X
  $ hg bookmarks -d Y
  $ hg summary --remote | grep '^remote:'
  remote: *, 2 incoming bookmarks, 1 outgoing bookmarks (glob)

  $ cd ..

pushing an unchanged bookmark should result in no changes

  $ hg init unchanged-a
  $ hg init unchanged-b
  $ cd unchanged-a
  $ echo initial > foo
  $ hg commit -A -m initial
  adding foo
  $ hg bookmark @
  $ hg push -B @ ../unchanged-b
  pushing to ../unchanged-b
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  exporting bookmark @

  $ hg push -B @ ../unchanged-b
  pushing to ../unchanged-b
  searching for changes
  no changes found
  [1]


Check hook preventing push (issue4455)
======================================

  $ hg bookmarks
   * @                         0:55482a6fb4b1
  $ hg log -G
  @  0:55482a6fb4b1 initial
  
  $ hg init ../issue4455-dest
  $ hg push ../issue4455-dest # changesets only
  pushing to ../issue4455-dest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > local=../issue4455-dest/
  > ssh=ssh://user@dummy/issue4455-dest
  > http=http://localhost:$HGPORT/
  > [ui]
  > ssh=python "$TESTDIR/dummyssh"
  > EOF
  $ cat >> ../issue4455-dest/.hg/hgrc << EOF
  > [hooks]
  > prepushkey=false
  > [web]
  > push_ssl = false
  > allow_push = *
  > EOF
  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS
  $ hg -R ../issue4455-dest serve -p $HGPORT -d --pid-file=../issue4455.pid -E ../issue4455-error.log
  $ cat ../issue4455.pid >> $DAEMON_PIDS

Local push
----------

  $ hg push -B @ local
  pushing to $TESTTMP/issue4455-dest (glob)
  searching for changes
  no changes found
  pushkey-abort: prepushkey hook exited with status 1
  exporting bookmark @ failed!
  [1]
  $ hg -R ../issue4455-dest/ bookmarks
  no bookmarks set

Using ssh
---------

  $ hg push -B @ ssh --config experimental.bundle2-exp=True
  pushing to ssh://user@dummy/issue4455-dest
  searching for changes
  no changes found
  remote: pushkey-abort: prepushkey hook exited with status 1
  exporting bookmark @ failed!
  [1]
  $ hg -R ../issue4455-dest/ bookmarks
  no bookmarks set

  $ hg push -B @ ssh --config experimental.bundle2-exp=False
  pushing to ssh://user@dummy/issue4455-dest
  searching for changes
  no changes found
  remote: pushkey-abort: prepushkey hook exited with status 1
  exporting bookmark @ failed!
  [1]
  $ hg -R ../issue4455-dest/ bookmarks
  no bookmarks set

Using http
----------

  $ hg push -B @ http --config experimental.bundle2-exp=True
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  remote: pushkey-abort: prepushkey hook exited with status 1
  exporting bookmark @ failed!
  [1]
  $ hg -R ../issue4455-dest/ bookmarks
  no bookmarks set

  $ hg push -B @ http --config experimental.bundle2-exp=False
  pushing to http://localhost:$HGPORT/
  searching for changes
  no changes found
  remote: pushkey-abort: prepushkey hook exited with status 1
  exporting bookmark @ failed!
  [1]
  $ hg -R ../issue4455-dest/ bookmarks
  no bookmarks set
