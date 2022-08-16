#chg-compatible
#debugruntest-compatible

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
