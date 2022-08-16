#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif
  $ setconfig experimental.allowfilepeer=True
  $ setconfig 'extensions.treemanifest=!'

# Set up upstream repo

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'share=' >> $HGRCPATH
  $ echo 'remotenames=' >> $HGRCPATH
  $ hg init upstream
  $ cd upstream
  $ touch file0
  $ hg add file0
  $ hg commit -m file0
  $ hg bookmark mainline
  $ cd ..

# Clone primary repo

  $ hg clone upstream primary
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd primary
  $ hg log --graph
  @  commit:      d26a60f4f448
     bookmark:    default/mainline (?)
     hoistedname: mainline (?)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     file0

# Share to secondary repo

  $ cd ..
  $ hg share -B primary secondary
  updating working directory
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd secondary
  $ hg log --graph
  @  commit:      d26a60f4f448
     bookmark:    default/mainline (?)
     hoistedname: mainline (?)
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     file0

# Check that tracking is also shared

  $ hg book local -t default/mainline
  $ hg book -v
   * local                     d26a60f4f448            [default/mainline]
  $ cd ../primary
  $ hg book -v
     local                     d26a60f4f448            [default/mainline]
