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
  @  9f17 commit 0

Even when it isn't the wdir

  $ hg -q up null

  $ hg show work
  o  9f17 commit 0

Single changeset is still there when public because it is a head

  $ hg phase --public -r 0
  $ hg show work
  o  9f17 commit 0

A draft child will show both it and public parent

  $ hg -q up 0
  $ echo 1 > foo
  $ hg commit -m 'commit 1'

  $ hg show work
  @  181c commit 1
  o  9f17 commit 0

Multiple draft children will be shown

  $ echo 2 > foo
  $ hg commit -m 'commit 2'

  $ hg show work
  @  128c commit 2
  o  181c commit 1
  o  9f17 commit 0

Bumping first draft changeset to public will hide its parent

  $ hg phase --public -r 1
  $ hg show work
  @  128c commit 2
  o  181c commit 1
  |
  ~

Multiple DAG heads will be shown

  $ hg -q up -r 1
  $ echo 3 > foo
  $ hg commit -m 'commit 3'
  created new head

  $ hg show work
  @  f0ab commit 3
  | o  128c commit 2
  |/
  o  181c commit 1
  |
  ~

Even when wdir is something else

  $ hg -q up null

  $ hg show work
  o  f0ab commit 3
  | o  128c commit 2
  |/
  o  181c commit 1
  |
  ~

Draft child shows public head (multiple heads)

  $ hg -q up 0
  $ echo 4 > foo
  $ hg commit -m 'commit 4'
  created new head

  $ hg show work
  @  668c commit 4
  | o  f0ab commit 3
  | | o  128c commit 2
  | |/
  | o  181c commit 1
  |/
  o  9f17 commit 0

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
  @  f8dd (mybranch) commit 4
  o  90cf (mybranch) commit 3
  | o  128c commit 2
  |/
  o  181c commit 1
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
  @  cac8 (mybook) commit 4
  o  f0ab commit 3
  | o  128c (@) commit 2
  |/
  o  181c commit 1
  |
  ~

  $ cd ..

Tags are rendered

  $ hg init tags
  $ cd tags
  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 1'
  $ echo 1 > foo
  $ hg commit -m 'commit 2'
  $ hg tag 0.1
  $ hg phase --public -r .
  $ echo 2 > foo
  $ hg commit -m 'commit 3'
  $ hg tag 0.2

  $ hg show work
  @  3758 Added tag 0.2 for changeset 6379c25b76f1
  o  6379 (0.2) commit 3
  o  a2ad Added tag 0.1 for changeset 6a75536ea0b1
  |
  ~

  $ cd ..

Multiple names on same changeset render properly

  $ hg init multiplenames
  $ cd multiplenames
  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 1'
  $ hg phase --public -r .
  $ hg branch mybranch
  marked working directory as branch mybranch
  (branches are permanent and global, did you want a bookmark?)
  $ hg bookmark mybook
  $ echo 1 > foo
  $ hg commit -m 'commit 2'

  $ hg show work
  @  3483 (mybook) (mybranch) commit 2
  o  97fc commit 1

Multiple bookmarks on same changeset render properly

  $ hg book mybook2
  $ hg show work
  @  3483 (mybook mybook2) (mybranch) commit 2
  o  97fc commit 1

  $ cd ..

Extra namespaces are rendered

  $ hg init extranamespaces
  $ cd extranamespaces
  $ echo 0 > foo
  $ hg -q commit -A -m 'commit 1'
  $ hg phase --public -r .
  $ echo 1 > foo
  $ hg commit -m 'commit 2'
  $ echo 2 > foo
  $ hg commit -m 'commit 3'

  $ hg --config extensions.revnames=$TESTDIR/revnamesext.py show work
  @  32f3 (r2) commit 3
  o  6a75 (r1) commit 2
  o  97fc (r0) commit 1

Obsolescence information appears in labels.

  $ cat >> .hg/hgrc << EOF
  > [experimental]
  > evolution=createmarkers
  > EOF
  $ hg debugobsolete `hg log -r 'desc("commit 2")' -T "{node}"`
  obsoleted 1 changesets
  $ hg show work --color=debug
  @  [log.changeset changeset.draft changeset.unstable instability.orphan|32f3] [log.description|commit 3]
  x  [log.changeset changeset.draft changeset.obsolete|6a75] [log.description|commit 2]
  |
  ~

  $ cd ..

Prefix collision on hashes increases shortest node length

  $ hg init hashcollision
  $ cd hashcollision
  $ echo 0 > a
  $ hg -q commit -Am 0
  $ for i in 17 1057 2857 4025; do
  >   hg -q up 0
  >   echo $i > a
  >   hg -q commit -m $i
  >   echo 0 > a
  >   hg commit -m "$i commit 2"
  > done

  $ hg show work
  @  cfd04 4025 commit 2
  o  c562d 4025
  | o  08048 2857 commit 2
  | o  c5623 2857
  |/
  | o  6a6b6 1057 commit 2
  | o  c5625 1057
  |/
  | o  96b4e 17 commit 2
  | o  11424 17
  |/
  o  b4e73 0

  $ cd ..
