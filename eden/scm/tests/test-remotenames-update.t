#chg-compatible

Set up repo

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=
  > EOF

  $ hg init repo
  $ cd repo
  $ echo 'foo'> a.txt
  $ hg add a.txt
  $ hg commit -m "a"
  $ echo 'bar' > b.txt
  $ hg add b.txt
  $ hg commit -m "b"
  $ hg bookmark foo -i
  $ echo 'bar' > c.txt
  $ hg add c.txt
  $ hg commit -q -m "c"

Testing update -B feature

  $ hg log -G -T '{rev} {bookmarks} {remotebookmarks}'
  @  2
  |
  o  1 foo
  |
  o  0
  

  $ hg update -B bar foo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark foo)
  $ hg log -G -T '{rev} {bookmarks} {remotebookmarks}'
  o  2
  |
  @  1 bar foo
  |
  o  0
  
  $ hg bookmarks -v
   * bar                       1:661086655130            [foo]
     foo                       1:661086655130

  $ hg update -B foo bar
  abort: bookmark 'foo' already exists
  [255]

Test that a bare update no long moves the active bookmark

  $ hg update
  updating to active bookmark bar
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} {bookmarks} {remotebookmarks}'
  o  2
  |
  @  1 bar foo
  |
  o  0
  
