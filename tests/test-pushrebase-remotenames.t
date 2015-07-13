  $ $PYTHON -c 'import remotenames' || exit 80
  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > [extensions]
  > remotenames =
  > pushrebase = $TESTDIR/../pushrebase.py
  > [remotenames]
  > allownonfastforward=True
  > EOF

Set up server repository

  $ hg init server
  $ cd server
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book master
  $ cd ..

Set up client repository

  $ hg clone ssh://user@dummy/server client -q

Test that pushing to a remotename gets rebased

  $ cd server
  $ hg up -q master
  $ echo x >> a && hg commit -m "master's commit"
  $ cd ../client
  $ echo x >> b && hg commit -m "client's commit"
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  1 "client's commit"
  |
  o  0 "initial" default/master
  

  $ hg push --to master
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  updating bookmark master

  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  3 "client's commit" default/master
  |
  o  2 "master's commit"
  |
  | @  1 "client's commit"
  |/
  o  0 "initial"
  

  $ cd ../server
  $ hg log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "client's commit" master
  |
  @  1 "master's commit"
  |
  o  0 "initial"
  
