#chg-compatible

  $ configure modern
  $ setconfig ui.allowemptycommit=1

  $ newrepo
  $ echo A | hg debugdrawdag

Active bookmark.

  $ hg up A -q

Read bookmark while updating it.

With metalog it works fine:

  $ hg log -r A -T '{desc}\n' --config hooks.pre-bookmark-load='hg commit -m A2'
  A

  $ hg log -r A -T '{desc}\n' --config hooks.pre-bookmark-load='hg commit -m A3'
  A2

Without metalog it causes errors:

  $ setconfig experimental.metalog=false
  $ hg log -r A -T '{desc}\n' --config hooks.pre-bookmark-load='hg commit -m A4'
  abort: unknown revision 'A'!
  (if A is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
