  $ . $TESTDIR/library.sh

setup configuration

  $ setup_config_repo

  $ cd $TESTTMP

setup repo

  $ hg init repo-hg
  $ cd repo-hg
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
  $ blobimport --blobstore files --linknodes repo-hg repo > /dev/null 2>&1

blobimport currently doesn't handle bookmarks, but server requires the directory.
  $ mkdir -p repo/books

Need a place for the socket to live
  $ mkdir -p repo/.hg

setup repo2

  $ hg clone repo-hg repo2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg pull ../repo-hg
  pulling from ../repo-hg
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
  
