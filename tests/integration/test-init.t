  $ . $TESTDIR/library.sh

setup configuration

  $ hg init mononoke-config
  $ cd mononoke-config
  $ mkdir repos
  $ cat > repos/repo <<CONFIG
  > path="$TESTTMP/repo"
  > repotype="revlog"
  > CONFIG
  $ hg add repos
  adding repos/repo
  $ hg ci -ma
  $ hg bookmark test-config
  $ hg log
  changeset:   0:* (glob)
  bookmark:    test-config
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ cd $TESTTMP

setup repo

  $ hg init repo
  $ cd repo
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log
  changeset:   0:3903775176ed
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  

  $ cd $TESTTMP

setup repo2

  $ hg clone repo repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg pull ../repo
  pulling from ../repo
  searching for changes
  no changes found

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ hgmn debugwireargs ssh://user@dummy/repo one two --three three
  one two three None None

  $ hgmn pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found

Create a new bookmark and try and send it over the wire
  $ cd ../repo
  $ hg bookmark test-bookmark
  $ hg bookmarks
   * test-bookmark             0:3903775176ed
  $ cd ../repo2
  $ hgmn pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding remote bookmark test-bookmark
  $ hg bookmarks
     test-bookmark             0:3903775176ed
