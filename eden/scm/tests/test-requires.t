#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ eagerepo
  $ setconfig experimental.allowfilepeer=True
  $ hg init t
  $ cd t
  $ echo a > a
  $ hg add a
  $ hg commit -m test
  $ rm .hg/requires
  $ hg tip
  abort: legacy dirstate implementations are no longer supported!
  [255]
  $ echo indoor-pool > .hg/requires
  $ hg tip
  abort: repository requires unknown features: indoor-pool
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ echo outdoor-pool >> .hg/requires
  $ hg tip
  abort: repository requires unknown features: indoor-pool, outdoor-pool
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
  $ cd ..
