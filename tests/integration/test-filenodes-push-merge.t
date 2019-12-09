
  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'


Creating a merge commit
  $ cd "$TESTTMP/repo2"
  $ hgmn up -q null
  $ echo 1 > tomerge
  $ hg -q addremove
  $ hg ci -m 'tomerge'
  $ NODE="$(hg log -r . -T '{node}')"
  $ hgmn up -q master_bookmark
  $ hgmn merge -q -r "$NODE"
  $ hg ci -m 'merge'

Pushing a merge
  $ hgmn push -r . --to master_bookmark
  pushing rev 7d332475050d to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  $ mononoke_admin filenodes validate "$(hg log -r master_bookmark -T '{node}')"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * changeset resolved as: * (glob)
