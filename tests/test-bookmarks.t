  $ hg init

no bookmarks

  $ hg bookmarks
  no bookmarks set

bookmark rev -1

  $ hg bookmark X

list bookmarks

  $ hg bookmarks
   * X                         -1:000000000000

list bookmarks with color

  $ hg --config extensions.color= --config color.mode=ansi \
  >    bookmarks --color=always
  \x1b[0;32m * X                         -1:000000000000\x1b[0m (esc)

  $ echo a > a
  $ hg add a
  $ hg commit -m 0

bookmark X moved to rev 0

  $ hg bookmarks
   * X                         0:f7b1eb17ad24

look up bookmark

  $ hg log -r X
  changeset:   0:f7b1eb17ad24
  bookmark:    X
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  

second bookmark for rev 0, command should work even with ui.strict on

  $ hg --config ui.strict=1 bookmark X2

bookmark rev -1 again

  $ hg bookmark -r null Y

list bookmarks

  $ hg bookmarks
     X                         0:f7b1eb17ad24
   * X2                        0:f7b1eb17ad24
     Y                         -1:000000000000

  $ echo b > b
  $ hg add b
  $ hg commit -m 1

bookmarks revset

  $ hg log -r 'bookmark()'
  changeset:   0:f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  changeset:   1:925d80f479bb
  bookmark:    X2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark(Y)'
  $ hg log -r 'bookmark(X2)'
  changeset:   1:925d80f479bb
  bookmark:    X2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark("re:X")'
  changeset:   0:f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  changeset:   1:925d80f479bb
  bookmark:    X2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark(unknown)'
  abort: bookmark 'unknown' does not exist
  [255]

  $ hg help revsets | grep 'bookmark('
      "bookmark([name])"

bookmarks X and X2 moved to rev 1, Y at rev -1

  $ hg bookmarks
     X                         0:f7b1eb17ad24
   * X2                        1:925d80f479bb
     Y                         -1:000000000000

bookmark rev 0 again

  $ hg bookmark -r 0 Z

  $ hg update X
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg add c
  $ hg commit -m 2
  created new head

bookmarks X moved to rev 2, Y at rev -1, Z at rev 0

  $ hg bookmarks
   * X                         2:db815d6d32e6
     X2                        1:925d80f479bb
     Y                         -1:000000000000
     Z                         0:f7b1eb17ad24

rename nonexistent bookmark

  $ hg bookmark -m A B
  abort: bookmark 'A' does not exist
  [255]

rename to existent bookmark

  $ hg bookmark -m X Y
  abort: bookmark 'Y' already exists (use -f to force)
  [255]

force rename to existent bookmark

  $ hg bookmark -f -m X Y

list bookmarks

  $ hg bookmark
     X2                        1:925d80f479bb
   * Y                         2:db815d6d32e6
     Z                         0:f7b1eb17ad24

bookmarks from a revset
  $ hg bookmark -r '.^1' REVSET
  $ hg bookmark -r ':tip' TIP
  $ hg up -q TIP
  $ hg bookmarks
     REVSET                    0:f7b1eb17ad24
   * TIP                       2:db815d6d32e6
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         0:f7b1eb17ad24

  $ hg bookmark -d REVSET
  $ hg bookmark -d TIP

rename without new name or multiple names

  $ hg bookmark -m Y
  abort: new bookmark name required
  [255]
  $ hg bookmark -m Y Y2 Y3
  abort: only one new bookmark name allowed
  [255]

delete without name

  $ hg bookmark -d
  abort: bookmark name required
  [255]

delete nonexistent bookmark

  $ hg bookmark -d A
  abort: bookmark 'A' does not exist
  [255]

bookmark name with spaces should be stripped

  $ hg bookmark ' x  y '

list bookmarks

  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         0:f7b1eb17ad24
   * x  y                      2:db815d6d32e6

look up stripped bookmark name

  $ hg log -r '"x  y"'
  changeset:   2:db815d6d32e6
  bookmark:    Y
  bookmark:    x  y
  tag:         tip
  parent:      0:f7b1eb17ad24
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  

reject bookmark name with newline

  $ hg bookmark '
  > '
  abort: bookmark names cannot consist entirely of whitespace
  [255]

  $ hg bookmark -m Z '
  > '
  abort: bookmark names cannot consist entirely of whitespace
  [255]

bookmark with reserved name

  $ hg bookmark tip
  abort: the name 'tip' is reserved
  [255]

  $ hg bookmark .
  abort: the name '.' is reserved
  [255]

  $ hg bookmark null
  abort: the name 'null' is reserved
  [255]


bookmark with existing name

  $ hg bookmark X2
  abort: bookmark 'X2' already exists (use -f to force)
  [255]

  $ hg bookmark -m Y Z
  abort: bookmark 'Z' already exists (use -f to force)
  [255]

bookmark with name of branch

  $ hg bookmark default
  abort: a bookmark cannot have the name of an existing branch
  [255]

  $ hg bookmark -m Y default
  abort: a bookmark cannot have the name of an existing branch
  [255]

bookmark with integer name

  $ hg bookmark 10
  abort: cannot use an integer as a name
  [255]

incompatible options

  $ hg bookmark -m Y -d Z
  abort: --delete and --rename are incompatible
  [255]

  $ hg bookmark -r 1 -d Z
  abort: --rev is incompatible with --delete
  [255]

  $ hg bookmark -r 1 -m Z Y
  abort: --rev is incompatible with --rename
  [255]

force bookmark with existing name

  $ hg bookmark -f X2

force bookmark back to where it was, should deactivate it

  $ hg bookmark -fr1 X2
  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         0:f7b1eb17ad24
     x  y                      2:db815d6d32e6

forward bookmark to descendant without --force

  $ hg bookmark Z
  moving bookmark 'Z' forward from f7b1eb17ad24

list bookmarks

  $ hg bookmark
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
   * Z                         2:db815d6d32e6
     x  y                      2:db815d6d32e6

revision but no bookmark name

  $ hg bookmark -r .
  abort: bookmark name required
  [255]

bookmark name with whitespace only

  $ hg bookmark ' '
  abort: bookmark names cannot consist entirely of whitespace
  [255]

  $ hg bookmark -m Y ' '
  abort: bookmark names cannot consist entirely of whitespace
  [255]

invalid bookmark

  $ hg bookmark 'foo:bar'
  abort: ':' cannot be used in a name
  [255]

  $ hg bookmark 'foo
  > bar'
  abort: '\n' cannot be used in a name
  [255]

the bookmark extension should be ignored now that it is part of core

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "bookmarks=" >> $HGRCPATH
  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
   * Z                         2:db815d6d32e6
     x  y                      2:db815d6d32e6

test summary

  $ hg summary
  parent: 2:db815d6d32e6 tip
   2
  branch: default
  bookmarks: *Z Y x  y
  commit: (clean)
  update: 1 new changesets, 2 branch heads (merge)

test id

  $ hg id
  db815d6d32e6 tip Y/Z/x  y

test rollback

  $ echo foo > f1
  $ hg ci -Amr
  adding f1
  $ hg bookmark -f Y -r 1
  $ hg bookmark -f Z -r 1
  $ hg rollback
  repository tip rolled back to revision 2 (undo commit)
  working directory now based on revision 2
  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         2:db815d6d32e6
     x  y                      2:db815d6d32e6

activate bookmark on working dir parent without --force

  $ hg bookmark --inactive Z
  $ hg bookmark Z

test clone

  $ hg bookmark -r 2 -i @
  $ hg bookmark -r 2 -i a@
  $ hg bookmarks
     @                         2:db815d6d32e6
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
   * Z                         2:db815d6d32e6
     a@                        2:db815d6d32e6
     x  y                      2:db815d6d32e6
  $ hg clone . cloned-bookmarks
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks bookmarks
   * @                         2:db815d6d32e6
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         2:db815d6d32e6
     a@                        2:db815d6d32e6
     x  y                      2:db815d6d32e6

test clone with pull protocol

  $ hg clone --pull . cloned-bookmarks-pull
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 3 files (+1 heads)
  updating to bookmark @
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks-pull bookmarks
   * @                         2:db815d6d32e6
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         2:db815d6d32e6
     a@                        2:db815d6d32e6
     x  y                      2:db815d6d32e6

delete multiple bookmarks at once

  $ hg bookmark -d @ a@

test clone with a bookmark named "default" (issue3677)

  $ hg bookmark -r 1 -f -i default
  $ hg clone . cloned-bookmark-default
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmark-default bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
     Z                         2:db815d6d32e6
     default                   1:925d80f479bb
     x  y                      2:db815d6d32e6
  $ hg -R cloned-bookmark-default parents -q
  2:db815d6d32e6
  $ hg bookmark -d default

test clone with a specific revision

  $ hg clone -r 925d80 . cloned-bookmarks-rev
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks-rev bookmarks
     X2                        1:925d80f479bb

test clone with update to a bookmark

  $ hg clone -u Z . cloned-bookmarks-update
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks-update bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
   * Z                         2:db815d6d32e6
     x  y                      2:db815d6d32e6

create bundle with two heads

  $ hg clone . tobundle
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo x > tobundle/x
  $ hg -R tobundle add tobundle/x
  $ hg -R tobundle commit -m'x'
  $ hg -R tobundle update -r -2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo y > tobundle/y
  $ hg -R tobundle branch test
  marked working directory as branch test
  (branches are permanent and global, did you want a bookmark?)
  $ hg -R tobundle add tobundle/y
  $ hg -R tobundle commit -m'y'
  $ hg -R tobundle bundle tobundle.hg
  searching for changes
  2 changesets found
  $ hg unbundle tobundle.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

update to current bookmark if it's not the parent

  $ hg summary
  parent: 2:db815d6d32e6 
   2
  branch: default
  bookmarks: [Z] Y x  y
  commit: 1 added, 1 unknown (new branch head)
  update: 2 new changesets (update)
  $ hg update
  updating to active bookmark Z
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         2:db815d6d32e6
   * Z                         3:125c9a1d6df6
     x  y                      2:db815d6d32e6

pull --update works the same as pull && update

  $ hg bookmark -r3 Y
  moving bookmark 'Y' forward from db815d6d32e6
  $ hg -R cloned-bookmarks-update update Y
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R cloned-bookmarks-update pull --update .
  pulling from .
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files (+1 heads)
  updating bookmark Y
  updating bookmark Z
  updating to active bookmark Y
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

test wrongly formated bookmark

  $ echo '' >> .hg/bookmarks
  $ hg bookmarks
     X2                        1:925d80f479bb
     Y                         3:125c9a1d6df6
   * Z                         3:125c9a1d6df6
     x  y                      2:db815d6d32e6
  $ echo "Ican'thasformatedlines" >> .hg/bookmarks
  $ hg bookmarks
  malformed line in .hg/bookmarks: "Ican'thasformatedlines"
     X2                        1:925d80f479bb
     Y                         3:125c9a1d6df6
   * Z                         3:125c9a1d6df6
     x  y                      2:db815d6d32e6

test missing revisions

  $ echo "925d80f479bc z" > .hg/bookmarks
  $ hg book
  no bookmarks set

test stripping a non-checked-out but bookmarked revision

  $ hg --config extensions.graphlog= log --graph
  o  changeset:   4:9ba5f110a0b3
  |  branch:      test
  |  tag:         tip
  |  parent:      2:db815d6d32e6
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     y
  |
  | @  changeset:   3:125c9a1d6df6
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     x
  |
  o  changeset:   2:db815d6d32e6
  |  parent:      0:f7b1eb17ad24
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     2
  |
  | o  changeset:   1:925d80f479bb
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1
  |
  o  changeset:   0:f7b1eb17ad24
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0
  
  $ hg book should-end-on-two
  $ hg co --clean 4
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg book four
  $ hg --config extensions.mq= strip 3
  saved backup bundle to * (glob)
should-end-on-two should end up pointing to revision 2, as that's the
tipmost surviving ancestor of the stripped revision.
  $ hg --config extensions.graphlog= log --graph
  @  changeset:   3:9ba5f110a0b3
  |  branch:      test
  |  bookmark:    four
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     y
  |
  o  changeset:   2:db815d6d32e6
  |  bookmark:    should-end-on-two
  |  parent:      0:f7b1eb17ad24
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     2
  |
  | o  changeset:   1:925d80f479bb
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1
  |
  o  changeset:   0:f7b1eb17ad24
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0
  

test clearing divergent bookmarks of linear ancestors

  $ hg bookmark Z -r 0
  $ hg bookmark Z@1 -r 1
  $ hg bookmark Z@2 -r 2
  $ hg bookmark Z@3 -r 3
  $ hg book
     Z                         0:f7b1eb17ad24
     Z@1                       1:925d80f479bb
     Z@2                       2:db815d6d32e6
     Z@3                       3:9ba5f110a0b3
   * four                      3:9ba5f110a0b3
     should-end-on-two         2:db815d6d32e6
  $ hg bookmark Z
  moving bookmark 'Z' forward from f7b1eb17ad24
  $ hg book
   * Z                         3:9ba5f110a0b3
     Z@1                       1:925d80f479bb
     four                      3:9ba5f110a0b3
     should-end-on-two         2:db815d6d32e6

test clearing only a single divergent bookmark across branches

  $ hg book foo -r 1
  $ hg book foo@1 -r 0
  $ hg book foo@2 -r 2
  $ hg book foo@3 -r 3
  $ hg book foo -r foo@3
  $ hg book
   * Z                         3:9ba5f110a0b3
     Z@1                       1:925d80f479bb
     foo                       3:9ba5f110a0b3
     foo@1                     0:f7b1eb17ad24
     foo@2                     2:db815d6d32e6
     four                      3:9ba5f110a0b3
     should-end-on-two         2:db815d6d32e6
