#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False

  $ setconfig extensions.treemanifest=! treemanifest.sendtrees=False treemanifest.treeonly=False
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc << 'EOF'
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo y >> x
  $ hg commit -qAm y
  $ echo z >> x
  $ hg commit -qAm z
  $ hg goto 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo w >> x
  $ hg commit -qAm w

  $ cd ..

# Shallow clone and activate getflogheads testing extension

  $ hgcloneshallow 'ssh://user@dummy/master' shallow --noupdate -q
  $ cd shallow

  $ cat >> .hg/hgrc << 'EOF'
  > [extensions]
  > getflogheads=$TESTDIR/getflogheads.py
  > EOF

# Get heads of a remotefilelog

  $ hg getflogheads x
  2797809ca5e9c2f307d82b1345e832f655fb99a2
  ca758b402ddc91e37e3113e1a97791b537e1b7bb

# Get heads of a non-existing remotefilelog

  $ hg getflogheads y
  EMPTY
