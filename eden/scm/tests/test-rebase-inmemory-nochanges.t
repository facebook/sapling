#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ enable amend rebase
  $ setconfig 'rebase.singletransaction=True'
  $ setconfig 'experimental.copytrace=off'
  $ setconfig 'rebase.experimental.inmemory=1'
  $ setconfig 'rebase.experimental.inmemory.nomergedriver=False'
  $ setconfig 'rebase.experimental.inmemorywarning=rebasing in-memory!'

  $ cd

# Create a commit with a move + content change:

  $ newrepo
  $ echo 'original content' > file
  $ hg add -q
  $ hg commit -q -m base
  $ echo 'new content' > file
  $ hg mv file file_new
  $ hg commit -m a
  $ hg book -r . a

# Recreate the same commit:

  $ hg up -q '.~1'
  $ echo 'new content' > file
  $ hg mv file file_new
  $ hg commit -m b
  $ hg book -r . b

  $ cp -R . ../without-imm

# Rebase one version onto the other, confirm it gets rebased out:

  $ hg rebase -r b -d a
  rebasing in-memory!
  rebasing 811ec875201f "b" (b)
  note: rebase of 811ec875201f created no changes to commit

# Without IMM, this behavior is semi-broken: the commit is not rebased out and the
# created commit is empty. (D8676355)

  $ cd ../without-imm

  $ setconfig 'rebase.experimental.inmemory=0'
  $ hg rebase -r b -d a
  rebasing 811ec875201f "b" (b)
  warning: can't find ancestor for 'file_new' copied from 'file'!

  $ hg export tip
  # HG changeset patch
  # User test
  # Date 0 0
  #      Thu Jan 01 00:00:00 1970 +0000
  # Node ID 0fe513c05d7fe2819c3ceccb072e74940604af36
  # Parent  24483d5afe6cb1a13b3642b4d8622e91f4d1bec1
  b
