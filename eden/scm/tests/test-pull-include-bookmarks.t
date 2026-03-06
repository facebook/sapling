#chg-compatible
#debugruntest-incompatible

  $ eagerepo
  $ setconfig remotenames.rename.default=
  $ setconfig remotenames.hoist=default

Set up remote repo with master and otherbookmark

  $ newclientrepo localrepo remoterepo
  $ cd ../remoterepo
  $ echo a > a
  $ hg add a
  $ hg commit -m 'First'
  $ hg book master

  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  $ hg book otherbookmark
  $ echo c > c
  $ hg add c
  $ hg commit -m 'Second'

  $ cd ../localrepo

Without config, pull -B only pulls the named bookmark
  $ hg pull -B otherbookmark
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/otherbookmark     * (glob)

With include-default-bookmarks, pull -B also pulls the selectivepulldefault bookmarks
  $ hg pull -B otherbookmark --config pull.include-default-bookmarks=True
  pulling from test:remoterepo
  imported commit graph for 1 commit (1 segment)
  $ hg bookmarks --list-subscriptions
     default/master            * (glob)
     default/otherbookmark     * (glob)

Default bookmarks already in -B do not cause duplication
  $ hg pull -B otherbookmark -B master --config pull.include-default-bookmarks=True
  pulling from test:remoterepo
  $ hg bookmarks --list-subscriptions
     default/master            * (glob)
     default/otherbookmark     * (glob)
