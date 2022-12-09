#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ hg init
  $ hg debugdrawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS

  $ setconfig 'devel.legacy.revnum=warn'

# use revnum directly

  $ hg log -r 0 -T '.\n'
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 0) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

# negative revnum

  $ hg goto -r -2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  hint[revnum-deprecate]: Local revision numbers (ex. -2) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

# revset operators

  $ hg log -r 1+2 -T '.\n'
  .
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 1) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

  $ hg log -r '::2' -T '.\n'
  .
  .
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 2) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

  $ hg log -r 2-1 -T '.\n'
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 2) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

# revset functions

  $ hg log -r 'parents(2)' -T '.\n'
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 2) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

  $ hg log -r 'sort(2+0)' -T '.\n'
  .
  .
  hint[revnum-deprecate]: Local revision numbers (ex. 2) are being deprecated and will stop working in the future. Please use commit hashes instead.
  hint[hint-ack]: use 'hg hint --ack revnum-deprecate' to silence these hints

# abort

  $ setconfig 'devel.legacy.revnum=abort'
  $ hg up 0
  abort: local revision number is disabled in this repo
  [255]

# smartlog revset

  $ enable smartlog
  $ hg log -r 'smartlog()' -T.
  ...
  $ hg log -r 'smartlog(1)' -T.
  abort: local revision number is disabled in this repo
  [255]

# phase

  $ hg phase
  112478962961147124edd43549aedd1a335e44bf: draft
