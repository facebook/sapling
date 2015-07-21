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

Resetting past a remote bookmark should not delete the remote bookmark

  $ cd ..
  $ hg clone repo client
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg book mybook -r 0
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg smartlog -T '{node|short} {bookmarks} {remotebookmarks}'
  o  a89d614e2364  default/master
  |
  @  b292c1e3311f mybook
  
  $ echo z >> x
  $ hg commit -qAm x3
  $ hg push -f --to master
  pushing rev d14b8058ba3a to destination $TESTTMP/repo bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating bookmark master
  $ hg smartlog -T '{node|short} {bookmarks} {remotebookmarks}'
  @  d14b8058ba3a  default/master
  |
  o  b292c1e3311f mybook
  
