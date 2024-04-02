# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#debugruntest-compatible

#require no-eden


# Empty update fails with a helpful error:

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'ui.disallowemptyupdate=True'
  $ configure modernclient
  $ newclientrepo
  $ hg debugdrawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up -q A
  $ hg up
  (If you're trying to move a bookmark forward, try "hg rebase -d <destination>".) (?)
  abort: you must specify a destination to update to, for example "hg goto main".
  [255]

# up -r works as intended:

  $ hg up -q -r B
  $ hg log -r . -T '{desc}\n'
  B
  $ hg up -q B
  $ hg log -r . -T '{desc}\n'
  B
