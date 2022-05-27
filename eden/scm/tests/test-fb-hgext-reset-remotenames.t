#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ configure modernclient
  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > reset=
  > remotenames=
  > EOF

  $ newclientrepo repo

  $ echo x > x
  $ hg commit -qAm x
  $ hg book foo
  $ echo x >> x
  $ hg commit -qAm x2
  $ hg push -q -r . --to foo --create

# Resetting past a remote bookmark should not delete the remote bookmark

  $ newclientrepo client test:repo_server foo
  $ hg book --list-remote *
  $ hg book bar
  $ hg reset --clean 'remote/foo^'
  $ hg log -G -T '{node|short} {bookmarks} {remotebookmarks}\n'
  o  a89d614e2364  remote/foo
  │
  @  b292c1e3311f bar
