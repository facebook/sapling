#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
# Copyright (c) Mercurial Contributors.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

  $ configure modernclient

# Create an empty repo:

  $ newclientrepo a

# Try some commands:

  $ hg log
  $ hg histgrep wah
  [1]
  $ hg manifest

# Poke at a clone:

  $ hg push -r . -q --to book --create

  $ cd ..
  $ newclientrepo b test:a_server
  $ hg log
