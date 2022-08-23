#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# TODO: Make this test compatibile with obsstore enabled.

  $ setconfig 'experimental.evolution='

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > rebase=
  > copytrace=
  > [experimental]
  > copytrace=off
  > EOF

  $ hg init repo
  $ cd repo
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m 1
  $ echo 2 > 1
  $ hg ci -m 2
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg mv 1 2
  $ hg ci -m dest
  $ hg rebase -s 1 -d .
  rebasing 812796267395 "2"
  other [source] changed 1 which local [dest] deleted
  hint: if this is due to a renamed file, you can manually input the renamed path, or re-run the command using --config=experimental.copytrace=on to make hg figure out renamed path automatically (which is very slow, and you will need to be patient)
  use (c)hanged version, leave (d)eleted, or leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted
  $ hg rebase -s 1 -d . --config=experimental.copytrace=on
  rebasing 812796267395 "2"
  merging 2 and 1 to 2
