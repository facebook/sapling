  $ hg init t
  $ cd t
  $ echo 1 > foo
  $ hg ci -Am m
  adding foo

  $ cd ..
  $ hg clone t tt
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tt
  $ echo 1.1 > foo
  $ hg ci -Am m

  $ cd ../t
  $ echo 1.2 > foo
  $ hg ci -Am m

Should not update to the other topological branch:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ cd ../tt

Should not update to the other branch:

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"

  $ HGMERGE=true hg merge
  merging foo
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -mm

  $ cd ../t

Should work:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (-1 heads)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Similarity between "hg update" and "hg pull -u" in handling bookmark
====================================================================

Test that updating activates the bookmark, which matches with the
explicit destination of the update.

  $ echo 4 >> foo
  $ hg commit -m "#4"
  $ hg bookmark active-after-pull
  $ cd ../tt

(1) activating by --rev BOOKMARK

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        3:483b76ad4309

  $ hg pull -u -r active-after-pull
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark active-after-pull
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark active-after-pull)

  $ hg parents -q
  4:f815b3da6163
  $ hg bookmarks
   * active-after-pull         4:f815b3da6163
     active-before-pull        3:483b76ad4309

(discard pulled changes)

  $ hg update -q 483b76ad4309
  $ hg rollback -q

(2) activating by URL#BOOKMARK

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        3:483b76ad4309

  $ hg pull -u $TESTTMP/t#active-after-pull
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  adding remote bookmark active-after-pull
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark active-after-pull)

  $ hg parents -q
  4:f815b3da6163
  $ hg bookmarks
   * active-after-pull         4:f815b3da6163
     active-before-pull        3:483b76ad4309

(discard pulled changes)

  $ hg update -q 483b76ad4309
  $ hg rollback -q

Test that updating deactivates current active bookmark, if the
destination of the update is explicitly specified, and it doesn't
match with the name of any exsiting bookmarks.

  $ cd ../t
  $ hg bookmark -d active-after-pull
  $ hg branch bar -q
  $ hg commit -m "#5 (bar #1)"
  $ cd ../tt

(1) deactivating by --rev REV

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        3:483b76ad4309

  $ hg pull -u -r b5e4babfaaa7
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark active-before-pull)

  $ hg parents -q
  5:b5e4babfaaa7
  $ hg bookmarks
     active-before-pull        3:483b76ad4309

(discard pulled changes)

  $ hg update -q 483b76ad4309
  $ hg rollback -q

(2) deactivating by --branch BRANCH

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        3:483b76ad4309

  $ hg pull -u -b bar
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark active-before-pull)

  $ hg parents -q
  5:b5e4babfaaa7
  $ hg bookmarks
     active-before-pull        3:483b76ad4309

(discard pulled changes)

  $ hg update -q 483b76ad4309
  $ hg rollback -q

(3) deactivating by URL#ANOTHER-BRANCH

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        3:483b76ad4309

  $ hg pull -u $TESTTMP/t#bar
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark active-before-pull)

  $ hg parents -q
  5:b5e4babfaaa7
  $ hg bookmarks
     active-before-pull        3:483b76ad4309

  $ cd ..
