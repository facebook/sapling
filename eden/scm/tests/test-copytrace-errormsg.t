
#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ enable rebase

  $ newclientrepo
  $ drawdag <<'EOS'
  > C   # C/y = 1\n (renamed from x)
  > |
  > | B # B/x = 2\n
  > | |
  > |/
  > A   # A/x = 1\n
  > EOS

  $ sl rebase -r $B -d $C --config copytrace.dagcopytrace=False
  rebasing 98114c1b9d02 "B"
  other [source] changed x which local [dest] is missing
  hint: if this is due to a renamed file, you can manually input the renamed path
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see sl resolve, then sl rebase --continue)
  [1]
  $ sl rebase --abort
  rebase aborted
  $ sl rebase -r $B -d $C --config copytrace.dagcopytrace=True
  rebasing 98114c1b9d02 "B"
  merging y and x to y
