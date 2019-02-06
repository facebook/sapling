  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > [extensions]
  > amend=
  > EOF

Setup helpers
  $ log() {
  >   hg sl -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
  > }

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

  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev a0c9c5791058 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

TODO(stash): pushrebase of a merge commit, pushrebase over a merge commit

  $ hgmn up master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ log -r ":"
  @  1 [public;rev=4;c2e526aacb51] default/master_bookmark
  |
  o  C [public;rev=2;26805aba1e60]
  |
  o  B [public;rev=1;112478962961]
  |
  | o  1 [draft;rev=3;a0c9c5791058]
  |/
  o  A [public;rev=0;426bada5c675]
   (re)


Push rebase fails with conflict in the bottom of the stack
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hgmn push -r . --to master_bookmark
  remote: * Session with Mononoke started with uuid: * (glob)
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: * pushrebase failed * (glob)
  remote:     msg: "pushrebase failed Conflicts([PushrebaseConflict { left: MPath(\"1\"), right: MPath(\"1\") }])"
  remote: * backtrace* (glob)
  abort: * (glob)
  [255]
  $ hg hide -r ".^ + ." -q


Push rebase fails with conflict in the top of the stack
  $ hg up -q 0
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  remote: * Session with Mononoke started with uuid: * (glob)
  pushing rev * to destination ssh://user@dummy/repo bookmark master_bookmark (glob)
  searching for changes
  remote: * pushrebase failed * (glob)
  remote:     msg: "pushrebase failed Conflicts([PushrebaseConflict { left: MPath(\"1\"), right: MPath(\"1\") }])"
  remote: * backtrace* (glob)
  abort: * (glob)
  [255]
  $ hg hide -r ".^ + ." -q


Push stack
  $ hg up -q 0
  $ echo 3 > 3 && hg add 3 && hg ci -m 3
  $ echo 4 > 4 && hg add 4 && hg ci -m 4
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 7a68f123d810 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ hg hide -r ".^ + ." -q
  $ hgmn up -q master_bookmark
  $ log -r ":"
  @  4 [public;rev=11;4f5a4463b24b] default/master_bookmark
  |
  o  3 [public;rev=10;7796136324ad]
  |
  o  1 [public;rev=4;c2e526aacb51]
  |
  o  C [public;rev=2;26805aba1e60]
  |
  o  B [public;rev=1;112478962961]
  |
  o  A [public;rev=0;426bada5c675]
   (re)
