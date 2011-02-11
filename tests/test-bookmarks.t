  $ echo "[extensions]" >> $HGRCPATH
  $ echo "bookmarks=" >> $HGRCPATH

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
  

second bookmark for rev 0

  $ hg bookmark X2

bookmark rev -1 again

  $ hg bookmark -r null Y

list bookmarks

  $ hg bookmarks
   * X                         0:f7b1eb17ad24
   * X2                        0:f7b1eb17ad24
     Y                         -1:000000000000

  $ echo b > b
  $ hg add b
  $ hg commit -m 1

bookmarks revset

  $ hg log -r 'bookmark()'
  changeset:   1:925d80f479bb
  bookmark:    X
  bookmark:    X2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg log -r 'bookmark(Y)'
  $ hg log -r 'bookmark(X2)'
  changeset:   1:925d80f479bb
  bookmark:    X
  bookmark:    X2
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     1
  
  $ hg help revsets | grep 'bookmark('
      "bookmark([name])"

bookmarks X and X2 moved to rev 1, Y at rev -1

  $ hg bookmarks
   * X                         1:925d80f479bb
   * X2                        1:925d80f479bb
     Y                         -1:000000000000

bookmark rev 0 again

  $ hg bookmark -r 0 Z

  $ echo c > c
  $ hg add c
  $ hg commit -m 2

bookmarks X and X2 moved to rev 2, Y at rev -1, Z at rev 0

  $ hg bookmarks
   * X                         2:0316ce92851d
   * X2                        2:0316ce92851d
     Y                         -1:000000000000
     Z                         0:f7b1eb17ad24

rename nonexistent bookmark

  $ hg bookmark -m A B
  abort: a bookmark of this name does not exist
  [255]

rename to existent bookmark

  $ hg bookmark -m X Y
  abort: a bookmark of the same name already exists
  [255]

force rename to existent bookmark

  $ hg bookmark -f -m X Y

list bookmarks

  $ hg bookmark
   * X2                        2:0316ce92851d
   * Y                         2:0316ce92851d
     Z                         0:f7b1eb17ad24

rename without new name

  $ hg bookmark -m Y
  abort: new bookmark name required
  [255]

delete without name

  $ hg bookmark -d
  abort: bookmark name required
  [255]

delete nonexistent bookmark

  $ hg bookmark -d A
  abort: a bookmark of this name does not exist
  [255]

bookmark name with spaces should be stripped

  $ hg bookmark ' x  y '

list bookmarks

  $ hg bookmarks
   * X2                        2:0316ce92851d
   * Y                         2:0316ce92851d
     Z                         0:f7b1eb17ad24
   * x  y                      2:0316ce92851d

look up stripped bookmark name

  $ hg log -r '"x  y"'
  changeset:   2:0316ce92851d
  bookmark:    X2
  bookmark:    Y
  bookmark:    x  y
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2
  

reject bookmark name with newline

  $ hg bookmark '
  > '
  abort: bookmark name cannot contain newlines
  [255]

bookmark with existing name

  $ hg bookmark Z
  abort: a bookmark of the same name already exists
  [255]

force bookmark with existing name

  $ hg bookmark -f Z

list bookmarks

  $ hg bookmark
   * X2                        2:0316ce92851d
   * Y                         2:0316ce92851d
   * Z                         2:0316ce92851d
   * x  y                      2:0316ce92851d

revision but no bookmark name

  $ hg bookmark -r .
  abort: bookmark name required
  [255]

bookmark name with whitespace only

  $ hg bookmark ' '
  abort: bookmark names cannot consist entirely of whitespace
  [255]
