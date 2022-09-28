# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Empty update fails with a helpful error:

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig 'ui.disallowemptyupdate=True'
  $ newrepo
  $ hg debugdrawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up -q 0
  $ hg up
  abort: You must specify a destination to update to, for example "hg update master".
  (If you're trying to move a bookmark forward, try "hg rebase -d <destination>".)
  [255]

# up -r works as intended:

  $ hg up -q -r 1
  $ hg log -r . -T '{rev}\n'
  1
  $ hg up -q 1
  $ hg log -r . -T '{rev}\n'
  1
