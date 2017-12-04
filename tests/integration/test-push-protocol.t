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

create a new commit in repo2 and check that it's seen as outgoing

  $ touch b
  $ hg add b
  $ hg ci -mb
  $ hg log
  changeset:   1:0e067c57feba
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ hgmn outgoing ssh://user@dummy/repo
  comparing with ssh://user@dummy/repo
  searching for changes
  changeset:   1:0e067c57feba
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
