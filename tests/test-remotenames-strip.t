  $ enable remotenames

Test that hg debugstrip -B stops at remotenames
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ echo a > a
  $ hg commit -Aqm aa
  $ hg phase -p
  $ hg push -q --to master --create
  $ echo b > b
  $ hg commit -Aqm bb
  $ hg book foo
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  bb (foo) ()
  aa () (default/master)
  a () ()
  $ hg debugstrip -qB foo
  bookmark 'foo' deleted
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  aa () (default/master)
  a () ()

Test that hg debugstrip -B deletes bookmark even if there is a remote bookmark,
but doesn't delete the commit.
  $ hg init server
  $ hg clone -q server client
  $ cd client
  $ echo x > x
  $ hg commit -Aqm a
  $ hg phase -p
  $ hg push -q --to master --create
  $ hg book foo
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  a (foo) (default/master)
  $ hg debugstrip -qB foo
  bookmark 'foo' deleted
  abort: empty revision set
  [255]
  $ hg log -T '{desc} ({bookmarks}) ({remotebookmarks})\n'
  a () (default/master)

