#debugruntest-compatible
# coding=utf-8
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.

  $ configure modernclient

  $ cat >> $HGRCPATH << 'EOF'
  > [extensions]
  > smartlog=
  > remotenames=
  > [commitcloud]
  > enablestatus=false
  > EOF

  $ newclientrepo repo

  $ echo x > x
  $ hg commit -qAm x1
  $ hg book master1
  $ echo x >> x
  $ hg commit -qAm x2
  $ hg push -r . -q --to master1 --create

# Non-bookmarked public heads should not be visible in smartlog

  $ newclientrepo client test:repo_server master1
  $ hg book mybook -r 'desc(x1)'
  $ hg up 'desc(x1)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  o  x2  remote/master1
  │
  @  x1 mybook

# Old head (rev 1) is still visible

  $ echo z >> x
  $ hg commit -qAm x3
  $ hg push --non-forward-move -q --to master1
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  @  x3  remote/master1
  │
  o  x1 mybook

# Test configuration of "interesting" bookmarks

  $ hg up -q '.^'
  $ echo x >> x
  $ hg commit -qAm x4
  $ hg push -q --to project/bookmark --create
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  o  x3  remote/master1
  │
  │ @  x4
  ├─╯
  o  x1 mybook

  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  o  x3  remote/master1
  │
  │ o  x4
  ├─╯
  @  x1 mybook
  $ cat >> $HGRCPATH << 'EOF'
  > [smartlog]
  > repos=default/
  > names=project/bookmark
  > EOF
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  o  x3  remote/master1
  │
  │ o  x4
  ├─╯
  @  x1 mybook
  $ cat >> $HGRCPATH << 'EOF'
  > [smartlog]
  > names=master project/bookmark
  > EOF
  $ hg smartlog -T '{desc} {bookmarks} {remotebookmarks}'
  o  x3  remote/master1
  │
  │ o  x4
  ├─╯
  @  x1 mybook
