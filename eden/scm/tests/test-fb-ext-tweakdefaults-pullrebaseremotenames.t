#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ configure modernclient

# Set up with remotenames

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > rebase=
  > remotenames=
  > tweakdefaults=
  > EOF

  $ newclientrepo repo
  $ cd ..
  $ echo a > repo/a
  $ hg -R repo commit -qAm a
  $ hg -R repo bookmark master
  $ hg -R repo push -q -r . --to book --create
  $ newclientrepo clone test:repo_server book

# Pull --rebase with no local changes

  $ hg bookmark localbookmark -t book
  $ echo b > ../repo/b
  $ hg -R ../repo commit -qAm b
  $ hg -R ../repo push -q -r . --to book
  $ hg pull --rebase
  pulling from test:repo_server
  searching for changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  nothing to rebase - fast-forwarded to book
  $ hg log -G -T '{desc}: {bookmarks}'
  @  b: localbookmark
  │
  o  a:

# Make a local commit and check pull --rebase still works.

  $ echo x > x
  $ hg commit -qAm x
  $ echo c > ../repo/c
  $ hg -R ../repo commit -qAm c
  $ hg -R ../repo push -q -r . --to book
  $ hg pull --rebase
  pulling from test:repo_server
  searching for changes
  rebasing 86d71924e1d0 "x" (localbookmark)
  $ hg log -G -T '{desc}: {bookmarks}'
  @  x: localbookmark
  │
  o  c:
  │
  o  b:
  │
  o  a:
