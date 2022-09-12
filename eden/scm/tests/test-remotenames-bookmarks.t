#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > remotenames=
  > EOF

# Setup repo

  $ hg init repo
  $ cd repo
  $ echo foo > a.txt
  $ hg add a.txt
  $ hg commit -m a

# Testing bookmark options without args

  $ hg bookmark a
  $ hg bookmark b
  $ hg bookmark -v
     a                         2dcb9139ea49
   * b                         2dcb9139ea49
  $ hg bookmark --track a
  $ hg bookmark -v
     a                         2dcb9139ea49
   * b                         2dcb9139ea49            [a]
  $ hg bookmark --untrack
  $ hg bookmark -v
     a                         2dcb9139ea49
   * b                         2dcb9139ea49
