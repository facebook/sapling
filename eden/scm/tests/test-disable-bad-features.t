
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test various flags to turn off bad sl features.

  $ eagerepo
  $ newrepo
  $ drawdag << 'EOS'
  > A
  > EOS
  $ sl up -Cq $A

# Test disabling the `sl merge` command:

  $ sl merge
  abort: nothing to merge
  [255]
  $ setconfig 'ui.allowmerge=False'
  $ sl merge
  abort: merging is not supported for this repository
  (use rebase, or 'sl merge --noconflict' for a conflict-free merge)
  [255]

# Test disabling the `sl branch` commands:
# Under SL identity, the `branch` command is not available at all.

  $ sl branch
  unknown command 'branch'
  hint: perhaps you'd like to use "sl bookmark".
  More info: https://sapling-scm.com/docs/overview/bookmarks
  [255]
  $ setconfig 'ui.allowbranches=False'
  $ sl branch foo
  unknown command 'branch'
  hint: perhaps you'd like to use "sl bookmark".
  More info: https://sapling-scm.com/docs/overview/bookmarks
  [255]
  $ setconfig 'ui.disallowedbrancheshint=use bookmarks instead! see docs'
  $ sl branch -C
  unknown command 'branch'
  hint: perhaps you'd like to use "sl bookmark".
  More info: https://sapling-scm.com/docs/overview/bookmarks
  [255]
