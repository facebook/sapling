#chg-compatible
#debugruntest-compatible
  $ configure modernclient
  $ newclientrepo repo

  $ cat > $TESTTMP/hook.sh <<'EOF'
  > echo "test-hook-bookmark: $HG_BOOKMARK:  $HG_OLDNODE -> $HG_NODE"
  > EOF
  $ TESTHOOK="hooks.txnclose-bookmark.test=sh $TESTTMP/hook.sh"

no bookmarks

  $ hg bookmarks
  no bookmarks set

  $ hg bookmarks -Tjson
  [
  ]

bookmark rev -1

  $ hg bookmark X --config "$TESTHOOK"
  test-hook-bookmark: X:   -> 0000000000000000000000000000000000000000

list bookmarks

  $ hg bookmarks
   * X                         000000000000

list bookmarks with color

  $ hg --config extensions.color= --config color.mode=ansi \
  >    bookmarks --color=always
  \x1b[32m * \x1b[39m\x1b[32mX\x1b[39m\x1b[32m                         000000000000\x1b[39m (esc)

  $ echo a > a
  $ hg add a
  $ hg commit -m 0 --config "$TESTHOOK"
  test-hook-bookmark: X:  0000000000000000000000000000000000000000 -> f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

bookmark X moved to rev 0

  $ hg bookmarks
   * X                         f7b1eb17ad24

look up bookmark

  $ hg log -r X
  commit:      f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  

second bookmark for rev 0, command should work even with ui.strict on

  $ hg --config ui.strict=1 bookmark X2 --config "$TESTHOOK"
  test-hook-bookmark: X2:   -> f7b1eb17ad24730a1651fccd46c43826d1bbc2ac

bookmark rev -1 again

  $ hg bookmark -r null Y

list bookmarks

  $ hg bookmarks
     X                         f7b1eb17ad24
   * X2                        f7b1eb17ad24
     Y                         000000000000

  $ echo b > b
  $ hg add b
  $ hg commit -m 1 --config "$TESTHOOK"
  test-hook-bookmark: X2:  f7b1eb17ad24730a1651fccd46c43826d1bbc2ac -> 925d80f479bb026b0fb3deb27503780b13f74123

  $ hg bookmarks -Tjson
  [
   {
    "active": false,
    "bookmark": "X",
    "node": "f7b1eb17ad24730a1651fccd46c43826d1bbc2ac"
   },
   {
    "active": true,
    "bookmark": "X2",
    "node": "925d80f479bb026b0fb3deb27503780b13f74123"
   },
   {
    "active": false,
    "bookmark": "Y",
    "node": "0000000000000000000000000000000000000000"
   }
  ]

bookmarks revset

  $ hg log -r 'bookmark()'
  commit:      f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  commit:      925d80f479bb
  bookmark:    X2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark(Y)'
  $ hg log -r 'bookmark(X2)'
  commit:      925d80f479bb
  bookmark:    X2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark("re:X")'
  commit:      f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  
  commit:      925d80f479bb
  bookmark:    X2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark("literal:X")'
  commit:      f7b1eb17ad24
  bookmark:    X
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     0
  

  $ hg log -r 'bookmark(unknown)'
  abort: bookmark 'unknown' does not exist!
  [255]
  $ hg log -r 'bookmark("literal:unknown")'
  abort: bookmark 'unknown' does not exist!
  [255]
  $ hg log -r 'bookmark("re:unknown")'
  abort: no bookmarks exist that match 'unknown'!
  [255]
  $ hg log -r 'present(bookmark("literal:unknown"))'
  $ hg log -r 'present(bookmark("re:unknown"))'

  $ hg help revsets | grep 'bookmark('
      "bookmark([name])"
      "remotebookmark([name])"

bookmarks X and X2 moved to rev 1, Y at rev -1

  $ hg bookmarks
     X                         f7b1eb17ad24
   * X2                        925d80f479bb
     Y                         000000000000

bookmark rev 0 again

  $ hg bookmark -r 'desc(0)' Z

  $ hg goto X
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark X)
  $ echo c > c
  $ hg add c
  $ hg commit -m 2

bookmarks X moved to rev 2, Y at rev -1, Z at rev 0

  $ hg bookmarks
   * X                         db815d6d32e6
     X2                        925d80f479bb
     Y                         000000000000
     Z                         f7b1eb17ad24

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

rename bookmark using .

  $ hg book rename-me
  $ hg book -m . renamed --config "$TESTHOOK"
  test-hook-bookmark: rename-me:  db815d6d32e69058eadefc8cffbad37675707975 -> 
  test-hook-bookmark: renamed:   -> db815d6d32e69058eadefc8cffbad37675707975
  $ hg bookmark
     X2                        925d80f479bb
     Y                         db815d6d32e6
     Z                         f7b1eb17ad24
   * renamed                   db815d6d32e6
  $ hg up -q Y
  $ hg book -d renamed --config "$TESTHOOK"
  test-hook-bookmark: renamed:  db815d6d32e69058eadefc8cffbad37675707975 -> 

rename bookmark using . with no active bookmark

  $ hg book rename-me
  $ hg book -i rename-me
  $ hg book -m . renamed
  abort: no active bookmark
  [255]
  $ hg up -q Y
  $ hg book -d rename-me

delete bookmark using .

  $ hg book delete-me
  $ hg book -d .
  $ hg bookmark
     X2                        925d80f479bb
     Y                         db815d6d32e6
     Z                         f7b1eb17ad24
  $ hg up -q Y

delete bookmark using . with no active bookmark

  $ hg book delete-me
  $ hg book -i delete-me
  $ hg book -d .
  abort: no active bookmark
  [255]
  $ hg up -q Y
  $ hg book -d delete-me

list bookmarks

  $ hg bookmark
     X2                        925d80f479bb
   * Y                         db815d6d32e6
     Z                         f7b1eb17ad24

bookmarks from a revset
  $ hg bookmark -r '.^1' REVSET
  $ hg bookmark -r ':tip' TIP
  $ hg up -q TIP
  $ hg bookmarks
     REVSET                    f7b1eb17ad24
   * TIP                       db815d6d32e6
     X2                        925d80f479bb
     Y                         db815d6d32e6
     Z                         f7b1eb17ad24

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

ensure bookmark names are deduplicated before deleting
  $ hg book delete-me
  $ hg book -d delete-me delete-me

bookmark name with spaces should be stripped

  $ hg bookmark ' x  y '

list bookmarks

  $ hg bookmarks
     X2                        925d80f479bb
     Y                         db815d6d32e6
     Z                         f7b1eb17ad24
   * x  y                      db815d6d32e6

look up stripped bookmark name

  $ hg log -r '"x  y"'
  commit:      db815d6d32e6
  bookmark:    Y
  bookmark:    x  y
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
  bookmark default matches a changeset hash
  (did you leave a -r out of an 'hg bookmark' command?)
  $ hg bookmark -f default
  $ hg book -d default

  $ hg bookmark -f -m Y default
  $ hg book -m default Y

bookmark with integer name

  $ hg bookmark 10
  abort: cannot use an integer as a name
  [255]

bookmark with a name that matches a node id
  $ hg bookmark 925d80f479bb db815d6d32e6 --config "$TESTHOOK"
  bookmark 925d80f479bb matches a changeset hash
  (did you leave a -r out of an 'hg bookmark' command?)
  bookmark db815d6d32e6 matches a changeset hash
  (did you leave a -r out of an 'hg bookmark' command?)
  test-hook-bookmark: 925d80f479bb:   -> db815d6d32e69058eadefc8cffbad37675707975
  test-hook-bookmark: db815d6d32e6:   -> db815d6d32e69058eadefc8cffbad37675707975
  $ hg bookmark -d 925d80f479bb
  $ hg bookmark -d db815d6d32e6

  $ cd ..

incompatible options

  $ cd repo

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

  $ hg bookmark -f X2 --config "$TESTHOOK"
  test-hook-bookmark: X2:  925d80f479bb026b0fb3deb27503780b13f74123 -> db815d6d32e69058eadefc8cffbad37675707975

force bookmark back to where it was, should deactivate it

  $ hg bookmark -fr'desc(1)' X2
  $ hg bookmarks
     X2                        925d80f479bb
     Y                         db815d6d32e6
     Z                         f7b1eb17ad24
     x  y                      db815d6d32e6

forward bookmark to descendant without --force

  $ hg bookmark Z
  moving bookmark 'Z' forward from f7b1eb17ad24

list bookmarks

  $ hg bookmark
     X2                        925d80f479bb
     Y                         db815d6d32e6
   * Z                         db815d6d32e6
     x  y                      db815d6d32e6

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
     X2                        925d80f479bb
     Y                         db815d6d32e6
   * Z                         db815d6d32e6
     x  y                      db815d6d32e6

test summary

  $ hg summary
  parent: db815d6d32e6 
   2
  bookmarks: *Z Y x  y
  commit: (clean)
  phases: 3 draft

test id

  $ hg id
  db815d6d32e6 Y/Z/x  y

  $ echo foo > f1

activate bookmark on working dir parent without --force

  $ hg bookmark --inactive Z
  $ hg bookmark Z

test clone

  $ hg bookmark -r 'desc(2)' -i @
  $ hg bookmark -r 'desc(2)' -i a@

delete multiple bookmarks at once

  $ hg bookmark -d @ a@

