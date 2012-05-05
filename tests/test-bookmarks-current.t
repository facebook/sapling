  $ hg init

no bookmarks

  $ hg bookmarks
  no bookmarks set

set bookmark X

  $ hg bookmark X

list bookmarks

  $ hg bookmark
   * X                         -1:000000000000

list bookmarks with color

  $ hg --config extensions.color= --config color.mode=ansi \
  >     bookmark --color=always
  \x1b[0;32m * X                         -1:000000000000\x1b[0m (esc)

update to bookmark X

  $ hg update X
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

list bookmarks

  $ hg bookmarks
   * X                         -1:000000000000

rename

  $ hg bookmark -m X Z

list bookmarks

  $ cat .hg/bookmarks.current
  Z (no-eol)
  $ cat .hg/bookmarks
  0000000000000000000000000000000000000000 Z
  $ hg bookmarks
   * Z                         -1:000000000000

new bookmark Y

  $ hg bookmark Y

list bookmarks

  $ hg bookmark
   * Y                         -1:000000000000
     Z                         -1:000000000000

commit

  $ echo 'b' > b
  $ hg add b
  $ hg commit -m'test'

list bookmarks

  $ hg bookmark
   * Y                         0:719295282060
     Z                         -1:000000000000

Verify that switching to Z updates the current bookmark:
  $ hg update Z
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmark
     Y                         0:719295282060
   * Z                         -1:000000000000

Switch back to Y for the remaining tests in this file:
  $ hg update Y
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

delete bookmarks

  $ hg bookmark -d Y
  $ hg bookmark -d Z

list bookmarks

  $ hg bookmark
  no bookmarks set

update to tip

  $ hg update tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

set bookmark Y using -r .

  $ hg bookmark -r . Y

list bookmarks

  $ hg bookmark
   * Y                         0:719295282060

set bookmark Z using -i

  $ hg bookmark -r . -i Z
  $ hg bookmarks
   * Y                         0:719295282060
     Z                         0:719295282060

deactivate current bookmark using -i

  $ hg bookmark -i Y
  $ hg bookmarks
     Y                         0:719295282060
     Z                         0:719295282060

  $ hg up -q Y
  $ hg bookmarks
   * Y                         0:719295282060
     Z                         0:719295282060

deactivate current bookmark while renaming

  $ hg bookmark -i -m Y X
  $ hg bookmarks
     X                         0:719295282060
     Z                         0:719295282060

bare update moves the active bookmark forward

  $ echo a > a
  $ hg ci -Am1
  adding a
  $ hg update X
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg bookmarks
   * X                         0:719295282060
     Z                         0:719295282060
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updating bookmark X
  $ hg bookmarks
   * X                         1:cc586d725fbe
     Z                         0:719295282060

test deleting .hg/bookmarks.current when explicitly updating
to a revision

  $ echo a >> b
  $ hg ci -m.
  $ hg up -q X
  $ test -f .hg/bookmarks.current

try to update to it again to make sure we don't
set and then unset it

  $ hg up -q X
  $ test -f .hg/bookmarks.current

  $ hg up -q 1
  $ test -f .hg/bookmarks.current
  [1]

when a bookmark is active, hg up -r . is
analogus to hg book -i <active bookmark>

  $ hg up -q X
  $ hg up -q .
  $ test -f .hg/bookmarks.current
  [1]
