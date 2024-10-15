#modern-config-incompatible

#require no-eden

# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2.


# Test that hg debugstrip -B stops at remotenames

  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ echo a > a
  $ hg commit -Aqm aa
  $ hg debugmakepublic
  $ hg push -q --to master --create
  $ echo b > b
  $ hg commit -Aqm bb
  $ hg book foo
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  bb (foo) ()
  aa () (public/a6e72781733c178cd290a07022bb6c8460749e7b remote/master)
  a () ()
  $ hg debugstrip -qB foo
  bookmark 'foo' deleted
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  aa () (public/a6e72781733c178cd290a07022bb6c8460749e7b remote/master)
  a () ()

# Test that hg debugstrip -B deletes bookmark even if there is a remote bookmark,
# but doesn't delete the commit.

  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ hg debugmakepublic
  $ hg push -q --to master --create
  $ hg book foo
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  a (foo) (public/770eb8fce608e2c55f853a8a5ea328b659d70616 remote/master)
  $ hg debugstrip -qB foo
  bookmark 'foo' deleted
  abort: empty revision set
  [255]
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  a () (public/770eb8fce608e2c55f853a8a5ea328b659d70616 remote/master)
