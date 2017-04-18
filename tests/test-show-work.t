  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > show =
  > EOF

  $ hg init repo0
  $ cd repo0

Command works on an empty repo

  $ hg show work

Single draft changeset shown

  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 0'

  $ hg show work
  @  9f171 commit 0

Even when it isn't the wdir

  $ hg -q up null

  $ hg show work
  o  9f171 commit 0

Single changeset is still there when public because it is a head

  $ hg phase --public -r 0
  $ hg show work
  o  9f171 commit 0

A draft child will show both it and public parent

  $ hg -q up 0
  $ echo 1 > foo
  $ hg commit -m 'commit 1'

  $ hg show work
  @  181cc commit 1
  o  9f171 commit 0

Multiple draft children will be shown

  $ echo 2 > foo
  $ hg commit -m 'commit 2'

  $ hg show work
  @  128c8 commit 2
  o  181cc commit 1
  o  9f171 commit 0

Bumping first draft changeset to public will hide its parent

  $ hg phase --public -r 1
  $ hg show work
  @  128c8 commit 2
  o  181cc commit 1
  |
  ~

Multiple DAG heads will be shown

  $ hg -q up -r 1
  $ echo 3 > foo
  $ hg commit -m 'commit 3'
  created new head

  $ hg show work
  @  f0abc commit 3
  | o  128c8 commit 2
  |/
  o  181cc commit 1
  |
  ~

Even when wdir is something else

  $ hg -q up null

  $ hg show work
  o  f0abc commit 3
  | o  128c8 commit 2
  |/
  o  181cc commit 1
  |
  ~

Draft child shows public head (multiple heads)

  $ hg -q up 0
  $ echo 4 > foo
  $ hg commit -m 'commit 4'
  created new head

  $ hg show work
  @  668ca commit 4
  | o  f0abc commit 3
  | | o  128c8 commit 2
  | |/
  | o  181cc commit 1
  |/
  o  9f171 commit 0

  $ cd ..

Branch name appears in output

  $ hg init branches
  $ cd branches
  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 0'
  $ echo 1 > foo
  $ hg commit -m 'commit 1'
  $ echo 2 > foo
  $ hg commit -m 'commit 2'
  $ hg phase --public -r .
  $ hg -q up -r 1
  $ hg branch mybranch
  marked working directory as branch mybranch
  (branches are permanent and global, did you want a bookmark?)
  $ echo 3 > foo
  $ hg commit -m 'commit 3'
  $ echo 4 > foo
  $ hg commit -m 'commit 4'

  $ hg show work
  @  f8dd3 (mybranch) commit 4
  o  90cfc (mybranch) commit 3
  | o  128c8 commit 2
  |/
  o  181cc commit 1
  |
  ~

  $ cd ..

Bookmark name appears in output

  $ hg init bookmarks
  $ cd bookmarks
  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 0'
  $ echo 1 > foo
  $ hg commit -m 'commit 1'
  $ echo 2 > foo
  $ hg commit -m 'commit 2'
  $ hg phase --public -r .
  $ hg bookmark @
  $ hg -q up -r 1
  $ echo 3 > foo
  $ hg commit -m 'commit 3'
  created new head
  $ echo 4 > foo
  $ hg commit -m 'commit 4'
  $ hg bookmark mybook

  $ hg show work
  @  cac82 (mybook) commit 4
  o  f0abc commit 3
  | o  128c8 (@) commit 2
  |/
  o  181cc commit 1
  |
  ~

  $ cd ..
