# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig format.use-segmented-changelog=false

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Try creating with a tag
  $ TAG=another_mainline
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg" --tag another_mainline
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 0, data: 0, tag: another_mainline, repo: repo (glob)
  * about to upload 1 entries, tag: another_mainline, repo: repo (glob)
  * inserting into streaming clone database, tag: another_mainline, repo: repo (glob)
  * current max chunk num is None, tag: another_mainline, repo: repo (glob)

  $ start_and_wait_for_mononoke_server
Clone - check that no bytes were transferred from streaming clone because no tags were used
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 0 bytes of data (glob)
  transferred 0 bytes in * seconds (*) (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ diff repo-streamclone/.hg/store/00changelog.i repo-hg/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo-hg/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select tag, idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by tag, chunk_num asc;"
  another_mainline|streaming_clone-chunk000000-d1de0dadf747295f0e1ea4db829b8e87437476f94cefcb948cd3b366b599d49e5a7c74b2777372b74c4962c513f71c72252bf673a8c880387ea84a5317abb14b-idx|streaming_clone-chunk000000-a5750ff674daa16106403d02aebff7d19ad96a33886c026427002f30c9eea7bac76387c4dd5f5c42a9e3ab1ecd9c9b5d3c2a079406e127146bddd9dcc8c63e23-data

Now clone with tag, make sure that streaming clone was used
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone-tag --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true --config stream_out_shallow.tag="$TAG"
  streaming all changes
  2 files to transfer, 357 bytes of data
  transferred 357 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
