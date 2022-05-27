#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

# Setup the repository

  $ hg init myrepo
  $ cd myrepo
  $ mkdir -p foo/bar-test foo/bartest
  $ echo a > foo/bar-test/a.txt
  $ echo b > foo/bartest/b.txt
  $ hg add .
  adding foo/bar-test/a.txt
  adding foo/bartest/b.txt
  $ hg commit -m Init

  $ mkdir foo/bar
  $ echo c > foo/bar/c.txt
  $ hg add .
  adding foo/bar/c.txt
  $ hg commit -m 'Add foo/bar/c.txt'

  $ hg diff -r .^ -r . --stat
   foo/bar/c.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
