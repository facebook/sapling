#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ newrepo
  $ hg d

  $ hg di --config alias.did=root

  $ hg debugf
  unknown command 'debugf'
  (use 'hg help' to get help)
  [255]
