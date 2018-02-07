  $ . $TESTDIR/library.sh

setup configuration
  $ setup_config_repo
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

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
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotefilelog=
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath
  > EOF
  $ hgcloneshallow ssh://user@dummy/repo-hg repo2 --noupdate
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 3903775176ed

  $ cd repo2
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > remotefilelog=
  > [treemanifest]
  > server=False
  > treeonly=True
  > [remotefilelog]
  > server=False
  > reponame=repo
  > EOF
  $ hg pull
  pulling from ssh://user@dummy/repo-hg
  searching for changes
  no changes found

  $ cd $TESTTMP
  $ cd repo-hg
  $ touch b
  $ hg add b
  $ hg ci -mb
  $ echo content > c
  $ hg add c
  $ hg ci -mc
  $ hg log
  changeset:   2:3e19bf519e9a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  $ cd ..
  $ blobimport --blobstore files --linknodes repo-hg repo > /dev/null 2>&1

blobimport currently doesn't handle bookmarks, but server requires the directory.
  $ mkdir -p repo/books

Need a place for the socket to live
  $ mkdir -p repo/.hg

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo
  $ hgmn debugwireargs ssh://user@dummy/repo one two --three three
  one two three None None

  $ cd repo2
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn pull ssh://user@dummy/repo --traceback
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 0e067c57feba:3e19bf519e9a
  (run 'hg update' to get a working copy)
  $ hg log -r '::3e19bf519e9a'
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
  changeset:   1:0e067c57feba
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   2:3e19bf519e9a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  $ ls
  a
  $ hg up 0e067c57feba
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  a
  b

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
