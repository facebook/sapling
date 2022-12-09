#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ configure modernclient
  $ setconfig commands.update.requiredest=True

#if fsmonitor
  $ setconfig workingcopy.ruststatus=False
#endif

# Test update.requiredest


  $ newclientrepo repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ hg up
  abort: you must specify a destination
  (for example: hg goto ".::")
  [255]
  $ hg up .
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGPLAIN=1 hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --config 'commands.update.requiredest=False' up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg push -q --to master --create

# Check update.requiredest interaction with pull --update

  $ newclientrepo clone test:repo_server

  $ cd ../repo
  $ echo a >> a
  $ hg commit -qAm aa
  $ hg push -q --to master
  $ cd ../clone
  $ hg pull --update
  abort: update destination required by configuration
  (use hg pull followed by hg goto DEST)
  [255]

  $ cd ..

