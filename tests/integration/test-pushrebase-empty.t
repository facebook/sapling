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
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate --config extensions.remotenames= -q
  $ cd repo2
  $ setup_hg_client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF

Push single empty commit
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg revert -r .^ 1
  $ hg commit --amend
  $ hg show
  changeset:   4:4d5799789652
  tag:         tip
  parent:      0:426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  1
  
  
  
  $ hgmn push -r . --to master_bookmark
  pushing rev 4d5799789652 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Push empty and non-empty commit in a stack
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg revert -r .^ 2
  $ hg commit --amend
  $ hgmn push -r . --to master_bookmark
  pushing rev 22c3c2036561 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Push stack of empty commits
  $ hgmn up -q tip
  $ echo 1 > 11 && hg add 11 && hg ci -m emptystack1
  $ hg revert -r .^ 11
  $ hg commit --amend
  $ echo 1 > 111 && hg add 111 && hg ci -m emptystack2
  $ hg revert -r .^ 111
  $ hg commit --amend
  $ hgmn push -r . --to master_bookmark
  pushing rev aeb4783bffb3 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
