  $ . $TESTDIR/library.sh

setup configuration

  $ hg init mononoke-config
  $ cd mononoke-config
  $ mkdir repos
  $ cat > repos/repo <<CONFIG
  > path="$TESTTMP/repo"
  > repotype="blob:files"
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
  $ wait_for_mononoke $TESTTMP/repo
  $ hgmn debugwireargs ssh://user@dummy/repo one two --three three
  one two three None None

  $ hgmn pull ssh://user@dummy/repo
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found

Create a new bookmark and try and send it over the wire
Test commented while we have no bookmark support in blobimport or easy method
to create a fileblob bookmark
#  $ cd ../repo
#  $ hg bookmark test-bookmark
#  $ hg bookmarks
#   * test-bookmark             0:3903775176ed
#  $ cd ../repo2
#  $ hgmn pull ssh://user@dummy/repo
#  pulling from ssh://user@dummy/repo
#  searching for changes
#  no changes found
#  adding remote bookmark test-bookmark
#  $ hg bookmarks
#     test-bookmark             0:3903775176ed
