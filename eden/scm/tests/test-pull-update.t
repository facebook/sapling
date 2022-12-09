#debugruntest-compatible
#chg-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.
#inprocess-hg-incompatible

  $ setconfig devel.segmented-changelog-rev-compat=true
#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

  $ setconfig experimental.allowfilepeer=True
  $ hg init t
  $ cd t
  $ echo 1 > foo
  $ hg ci -Am m
  adding foo

  $ cd ..
  $ hg clone --no-shallow t tt
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tt
  $ echo 1.1 > foo
  $ hg ci -Am m

  $ cd ../t
  $ echo 1.2 > foo
  $ hg ci -Am m

# Should respect config to disable dirty update

  $ hg co -qC 0
  $ echo 2 > foo
  $ hg --config 'commands.update.check=abort' pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  abort: uncommitted changes
  [255]
  $ hg debugstrip --no-backup tip
  $ hg co -qC tip

# Should not update to the other topological branch:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "800c91d5bfc1: m"
  1 other heads for branch "default"

  $ cd ../tt

# Should not update to the other branch:

  $ hg pull -u ../t
  pulling from ../t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "107cefe13e42: m"
  1 other heads for branch "default"

  $ HGMERGE=true hg merge
  merging foo
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -mm

  $ cd ../t

# Should work:

  $ hg pull -u ../tt
  pulling from ../tt
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Similarity between "hg update" and "hg pull -u" in handling bookmark
# ====================================================================
# Test that updating activates the bookmark, which matches with the
# explicit destination of the update.

  $ echo 4 >> foo
  $ hg commit -m '#4'
  $ hg bookmark active-after-pull
  $ cd ../tt

# (1) activating by --rev BOOKMARK

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        483b76ad4309

  $ cp -R . $TESTTMP/tt-1
  $ cd $TESTTMP/tt-1

  $ hg pull -u -r active-after-pull
  pulling from $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding remote bookmark active-after-pull
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark active-after-pull)

  $ hg parents -q
  f815b3da6163
  $ hg bookmarks
   * active-after-pull         f815b3da6163
     active-before-pull        483b76ad4309

# (discard pulled changes)

  $ cd $TESTTMP/tt

# (2) activating by URL#BOOKMARK

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        483b76ad4309

  $ cp -R . $TESTTMP/tt-2
  $ cd $TESTTMP/tt-2

  $ hg pull -u "$TESTTMP/t#active-after-pull"
  pulling from $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  adding remote bookmark active-after-pull
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark active-after-pull)

  $ hg parents -q
  f815b3da6163
  $ hg bookmarks
   * active-after-pull         f815b3da6163
     active-before-pull        483b76ad4309

# (discard pulled changes)

  $ cd $TESTTMP/tt
  $ hg goto -q 483b76ad4309

# Test that updating deactivates current active bookmark, if the
# destination of the update is explicitly specified, and it doesn't
# match with the name of any existing bookmarks.

  $ cd ../t
  $ hg bookmark -d active-after-pull
  $ hg commit -m '#5 (bar #1)' --config 'ui.allowemptycommit=1'
  $ cd ../tt

# (1) deactivating by --rev REV

  $ hg bookmark -f active-before-pull
  $ hg bookmarks
   * active-before-pull        483b76ad4309

  $ hg pull -u -r f815b3da6163
  pulling from $TESTTMP/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (leaving bookmark active-before-pull)

  $ hg parents -q
  f815b3da6163
  $ hg bookmarks
     active-before-pull        483b76ad4309

# (discard pulled changes)

  $ hg goto -q 483b76ad4309

  $ cd ..
