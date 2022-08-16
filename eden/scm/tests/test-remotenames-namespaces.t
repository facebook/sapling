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

# Set up extension and repos

  $ echo '[extensions]' >> $HGRCPATH
  $ echo 'remotenames=' >> $HGRCPATH
  $ echo 'color=' >> $HGRCPATH
  $ echo '[color]' >> $HGRCPATH
  $ echo 'log.remotebookmark = yellow' >> $HGRCPATH
  $ echo 'log.remotebranch = red' >> $HGRCPATH
  $ echo 'log.hoistedname = blue' >> $HGRCPATH
  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -qm a
  $ hg boo bm2
  $ cd ..
  $ hg clone repo1 repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg bookmark local

# Test colors

  $ hg log '--color=always' -l 1
  [0;*mcommit:      cb9a9f314b8b[0m (glob)
  bookmark:    local
  [0;33mbookmark:    default/bm2[0m (?)
  [0;34mhoistedname: bm2[0m (?)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
