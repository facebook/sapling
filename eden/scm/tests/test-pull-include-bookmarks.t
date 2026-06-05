#chg-compatible

  $ eagerepo
  $ setconfig remotenames.rename.default=
  $ setconfig remotenames.hoist=default

Set up remote repo with master and otherbookmark

  $ newclientrepo localrepo remoterepo
  $ cd ../remoterepo
  $ echo a > a
  $ sl add a
  $ sl commit -m 'First'
  $ sl book master

  $ sl up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (leaving bookmark master)
  $ sl book otherbookmark
  $ echo c > c
  $ sl add c
  $ sl commit -m 'Second'

  $ cd ../localrepo

Without config, pull -B only pulls the named bookmark
  $ sl pull -B otherbookmark
  pulling from test:remoterepo
  $ sl bookmarks --list-subscriptions
     default/otherbookmark     * (glob)

With include-default-bookmarks, pull -B also pulls the selectivepulldefault bookmarks
  $ sl pull -B otherbookmark --config pull.include-default-bookmarks=True
  pulling from test:remoterepo
  imported commit graph for 1 commit (1 segment)
  $ sl bookmarks --list-subscriptions
     default/master            * (glob)
     default/otherbookmark     * (glob)

Default bookmarks already in -B do not cause duplication
  $ sl pull -B otherbookmark -B master --config pull.include-default-bookmarks=True
  pulling from test:remoterepo
  $ sl bookmarks --list-subscriptions
     default/master            * (glob)
     default/otherbookmark     * (glob)
