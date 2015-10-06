Set up repo

  $ extpath=`dirname $TESTDIR`
  $ cp $extpath/remotenames.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > remotenames=$TESTTMP/remotenames.py
  > EOF

  $ hg init repo
  $ cd repo
  $ echo 'foo'> a.txt
  $ hg add a.txt
  $ hg commit -m "a"
  $ echo 'bar' > b.txt
  $ hg add b.txt
  $ hg commit -m "b"
  $ hg bookmark foo
  $ hg update -q ".^"
  $ echo 'bar' > c.txt
  $ hg add c.txt
  $ hg commit -q -m "c"

Testing update -B feature

  $ hg log -G -T '{rev} {bookmarks} {remotebookmarks}'
  @  2
  |
  | o  1 foo
  |/
  o  0
  

  $ hg update -B bar foo
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  (activating bookmark foo)
  $ hg log -G -T '{rev} {bookmarks} {remotebookmarks}'
  o  2
  |
  | @  1 bar foo
  |/
  o  0
  
  $ hg bookmarks -v
   * bar                       1:661086655130            [foo]
     foo                       1:661086655130

  $ hg update -B foo bar
  abort: bookmark 'foo' already exists
  [255]
