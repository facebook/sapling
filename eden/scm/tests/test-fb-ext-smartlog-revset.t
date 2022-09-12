#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ newrepo
  $ enable smartlog
  $ drawdag << 'EOS'
  > B C  # B has date 100000 0
  > |/   # C has date 200000 0
  > A
  > EOS
  $ hg bookmark -ir "$A" master
  $ hg log -r 'smartlog()' -T '{desc}\n'
  A
  B
  C
  $ hg log -r "smartlog($B)" -T '{desc}\n'
  A
  B
  $ hg log -r "smartlog(heads=$C, master=$B)" -T '{desc}\n'
  A
  B
  C
  $ hg log -r "smartlog(master=($A::)-$B-$C)" -T '{desc}\n'
  A
  B
  C
