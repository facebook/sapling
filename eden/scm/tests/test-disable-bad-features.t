#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Test various flags to turn off bad hg features.

  $ newrepo
  $ drawdag << 'EOS'
  > A
  > EOS
  $ hg up -Cq $A

# Test disabling the `hg merge` command:

  $ hg merge
  abort: nothing to merge
  [255]
  $ setconfig 'ui.allowmerge=False'
  $ hg merge
  abort: merging is not supported for this repository
  (use rebase instead)
  [255]

# Test disabling the `hg branch` commands:

  $ hg branch
  default
  hint[branch-command-deprecate]: 'hg branch' command does not do what you want, and is being removed. It always prints 'default' for now. Check fburl.com/why-no-named-branches for details.
  hint[hint-ack]: use 'hg hint --ack branch-command-deprecate' to silence these hints
  $ setconfig 'ui.allowbranches=False'
  $ hg branch foo
  abort: named branches are disabled in this repository
  (use bookmarks instead)
  [255]
  $ setconfig 'ui.disallowedbrancheshint=use bookmarks instead! see docs'
  $ hg branch -C
  abort: named branches are disabled in this repository
  (use bookmarks instead! see docs)
  [255]
