#debugruntest-compatible

#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ eagerepo
  $ enable amend rebase
  $ setconfig 'rebase.singletransaction=True'
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
  note: not rebasing 811ec875201f, its destination (rebasing onto) commit already has all its changes

# Without IMM, confirm empty commit issue (D8676355) is fixed

  $ cd ../without-imm

  $ setconfig 'rebase.experimental.inmemory=0'
  $ setconfig 'copytrace.skipduplicatecopies=True'
  $ hg rebase -r b -d a
  rebasing 811ec875201f "b" (b)
  note: not rebasing 811ec875201f, its destination (rebasing onto) commit already has all its changes
