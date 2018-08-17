  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF

setup repo
  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ hg debugdrawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Clone the repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  pushing to ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes

TODO(stash): pushrebase of a merge commit, pushrebase over a merge commit

  $ hgmn pull -q
  $ hgmn up master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)
  $ hg sl -r ":"
  @  changeset:   4:c2e526aacb51
  |  bookmark:    master_bookmark
  |  tag:         tip
  |  parent:      2:26805aba1e60
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     1
  |
  o  changeset:   2:26805aba1e60
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     C
  |
  o  changeset:   1:112478962961
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B
  |
  | o  changeset:   3:a0c9c5791058
  |/   parent:      0:426bada5c675
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1
  |
  o  changeset:   0:426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  




TODO(stash, aslpavel) This push should fail because of the conflicts
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  pushing to ssh://user@dummy/repo
  remote: * Session with Mononoke started with uuid: * (glob)
  searching for changes

Push stack
  $ hg up -q 0
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ echo 3 > 3 && hg add 3 && hg ci -m 3
  $ hgmn push -r . --to master_bookmark
  pushing to ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  $ hgmn pull -q
  $ hgmn up -q master_bookmark
  $ hg sl -r ":"
  @  changeset:   9:87a1ed50f395
  |  bookmark:    master_bookmark
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     3
  |
  o  changeset:   8:9a63b5318212
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     2
  |
  o  changeset:   7:f45edac0a57f
  |  parent:      4:c2e526aacb51
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     1
  |
  o  changeset:   4:c2e526aacb51
  |  parent:      2:26805aba1e60
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     1
  |
  o  changeset:   2:26805aba1e60
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     C
  |
  o  changeset:   1:112478962961
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     B
  |
  | o  changeset:   6:3953a5b36168
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     3
  | |
  | o  changeset:   5:c9b2673d3218
  |/   parent:      0:426bada5c675
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     2
  |
  | o  changeset:   3:a0c9c5791058
  |/   parent:      0:426bada5c675
  |    user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1
  |
  o  changeset:   0:426bada5c675
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     A
  
