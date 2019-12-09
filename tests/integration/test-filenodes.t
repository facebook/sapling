  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_common_config "blob_files"
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
  >   hg log -G -T "{desc} [{phase};rev={rev};{node|short}] {remotenames}" "$@"
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

Pushrebase commit 1
  $ hg up -q 0
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hgmn push -r . --to master_bookmark
  pushing rev a0c9c5791058 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

Remove all filenodes, make sure that update still works fine
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes where repo_id >= 0";
  $ hgmn up -q master_bookmark
