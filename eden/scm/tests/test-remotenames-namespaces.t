#debugruntest-compatible
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ eagerepo

  $ setconfig experimental.allowfilepeer=True
  $ setconfig remotenames.hoist=default
  $ setconfig remotenames.rename.default=

# Set up extension and repos

  $ enable color
  $ setconfig color.log.remotebookmark=yellow color.log.remotebranch=red color.log.hoistedname=blue

  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg add a
  $ hg commit -qm a
  $ hg boo bm2
  $ cd ..
  $ newclientrepo repo2 test:repo1 bm2
  $ hg bookmark local

# Test colors

  $ hg log '--color=always' -l 1
  \x1b[33mcommit:      cb9a9f314b8b\x1b[39m (esc)
  bookmark:    local
  \x1b[0;33mbookmark:    default/bm2\x1b[0m (esc) (?)
  \x1b[0;34mhoistedname: bm2\x1b[0m (esc) (?)
  \x1b[33mbookmark:    default/bm2\x1b[39m (esc)
  \x1b[34mhoistedname: bm2\x1b[39m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
