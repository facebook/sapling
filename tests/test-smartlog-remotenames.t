  $ $PYTHON -c 'import remotenames' || exit 80
  $ extpath=$(dirname $TESTDIR)
  $ cp $extpath/smartlog.py $TESTTMP # use $TESTTMP substitution in message
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > smartlog=$TESTTMP/smartlog.py
  > remotenames=
  > EOF

  $ hg init repo
  $ cd repo

  $ echo x > x
  $ hg commit -qAm x
  $ hg book master
  $ echo x >> x
  $ hg commit -qAm x2

Non-bookmarked public heads should not be visible in smartlog

  $ cd ..
  $ hg clone repo client
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg book mybook -r 0
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  o  1  default/master
  |
  @  0 mybook
  
Old head (rev 1) should no longer be visible

  $ echo z >> x
  $ hg commit -qAm x3
  $ hg push -f -q --to master
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  @  2  default/master
  |
  o  0 mybook
  

Test configuration of "interesting" bookmarks

  $ hg up -q .^
  $ echo x >> x
  $ hg commit -qAm x4
  $ hg push -f -q --to project/bookmark --force
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  o  2  default/master
  |
  | @  3  default/project/bookmark
  |/
  o  0 mybook
  

  $ hg up .^
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  o  2  default/master
  |
  @  0 mybook
  
  $ cat >> $HGRCPATH << EOF
  > [smartlog]
  > repos=default/
  > names=project/bookmark
  > EOF
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  o  3  default/project/bookmark
  |
  @  0 mybook
  
  $ cat >> $HGRCPATH << EOF
  > [smartlog]
  > names=master project/bookmark
  > EOF
  $ hg smartlog -T '{rev} {bookmarks} {remotebookmarks}'
  o  2  default/master
  |
  | o  3  default/project/bookmark
  |/
  @  0 mybook
  
