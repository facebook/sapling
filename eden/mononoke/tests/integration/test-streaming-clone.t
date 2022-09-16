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

  $ LOG_FILE="$TESTTMP/log_file"
  $ streaming_clone --scuba-dataset dataset --scuba-log-file "$LOG_FILE" create --dot-hg-path "$TESTTMP/repo-hg/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 0, data: 0, repo: repo (glob)
  * about to upload 1 entries, repo: repo (glob)
  * inserting into streaming clone database, repo: repo (glob)
  * current max chunk num is None, repo: repo (glob)
  $ jq .normal.reponame < "$LOG_FILE"
  "repo"
  $ jq .normal.chunks_inserted < "$LOG_FILE"
  "1"

Try creating again, this should fail
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * cannot create new streaming clone chunks because they already exists (glob)
  [1]

  $ start_and_wait_for_mononoke_server
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
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

  $ diff repo-streamclone/.hg/store/00changelog.i repo-hg/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo-hg/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-d1de0dadf747295f0e1ea4db829b8e87437476f94cefcb948cd3b366b599d49e5a7c74b2777372b74c4962c513f71c72252bf673a8c880387ea84a5317abb14b-idx|streaming_clone-chunk000000-a5750ff674daa16106403d02aebff7d19ad96a33886c026427002f30c9eea7bac76387c4dd5f5c42a9e3ab1ecd9c9b5d3c2a079406e127146bddd9dcc8c63e23-data

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg" --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 0, data: 0, repo: repo (glob)
  * about to upload 3 entries, repo: repo (glob)
  * inserting into streaming clone database, repo: repo (glob)
  * current max chunk num is None, repo: repo (glob)
  $ rm -rf "$TESTTMP/repo-streamclone"
  $ cd "$TESTTMP"
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
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
  $ diff repo-streamclone/.hg/store/00changelog.i repo-hg/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo-hg/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-760d25b269f0be9a5ef3aabd126ef025d5f13d279da46d9721d8a07423dc9ba03be1acceb23e6f0ccd9bdc330bc2911ff386f1444cdf279ee0506368013792be-idx|streaming_clone-chunk000000-31d5f335f6e9ac058258e7d242402d6d0f218f075647b8aa9caee655127f66b1954236f46b1f0c19cf837ff9a80651f4f5681ace3bea083437f310d2ef92cf3e-data
  streaming_clone-chunk000001-ddc1b4ac17d56e27b899602bca51925d3fdfd21a1defc05ecf83c1d7b3ef2e0c4c9a3cb3a6e412936019888259d39b62f89bafe0af101f29d0eb189b9b528cfd-idx|streaming_clone-chunk000001-ff9763a4f2f9bce3bef31a9e03814d59e6d78b77371024d9b613f8a5829efe21d75fbf7374f1b7219c87ece7805246b7c0a74128b9d48f84e8840bf6ebf65249-data
  streaming_clone-chunk000002-b1d37871e4f34fab6d57b7bf076da07028a69d1a70e4bccca0721013e5eeaa3a2fcdf61785b808b8ecee68a69782be16bdee4f51d94eeb3bf82b10d7796a55b2-idx|streaming_clone-chunk000002-a4daa490eef701a7b0e2bd86eb1579500011a67f7d1eaf148bff6b1095dcd3278e8e0bf940b1a5b8db4af990d5edbed0f752c7513f50627b206749104c71ab4b-data

Push a few new commits and update streaming clone
  $ cd "$TESTTMP"
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ cd repo-push
  $ enableextension remotenames
  $ hgmn up tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 1 > 1
  $ hg add 1
  $ hg ci -m 'add 1'
  $ hgmn push -r . --to master_bookmark
  pushing rev abfb584eacfc to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ echo 11 > 1
  $ hg ci -m 'echo 11'
  $ hgmn push -r . --to master_bookmark
  pushing rev 3f2a7f32ccfc to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ echo 111 > 1
  $ hg ci -m 'echo 111'
  $ hgmn push -r . --to master_bookmark
  pushing rev 9e0f64de2fee to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark
  $ hg log -r tip
  commit:      9e0f64de2fee
  bookmark:    default/master_bookmark
  hoistedname: master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     echo 111
  

  $ cd "$TESTTMP"
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone-2 --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 357 bytes of data
  transferred 357 bytes in * seconds (* KB/sec) (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check that with last chunk skipping no new batches are uploaded
  $ streaming_clone update --dot-hg-path "$TESTTMP/repo-streamclone-2/.hg" --skip-last-chunk
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 192, data: 165, repo: repo (glob)
  * about to upload 0 entries, repo: repo (glob)
  * inserting into streaming clone database, repo: repo (glob)
  * current max chunk num is Some(2), repo: repo (glob)

  $ streaming_clone update --dot-hg-path "$TESTTMP/repo-streamclone-2/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 192, data: 165, repo: repo (glob)
  * about to upload 1 entries, repo: repo (glob)
  * inserting into streaming clone database, repo: repo (glob)
  * current max chunk num is Some(2), repo: repo (glob)

Clone it again to make sure saved streaming chunks are valid
  $ cd "$TESTTMP"
  $ hgmn clone --stream mononoke://$(mononoke_address)/repo repo-streamclone-3 --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 731 bytes of data
  transferred 731 bytes in 0.0 seconds (* KB/sec) (glob)
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo-streamclone-3
  $ hg log -r tip
  commit:      9e0f64de2fee
  bookmark:    master_bookmark
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     echo 111
  
  $ cd "$TESTTMP"
  $ diff repo-streamclone-2/.hg/store/00changelog.i repo-streamclone-3/.hg/store/00changelog.i
  $ diff repo-streamclone-2/.hg/store/00changelog.d repo-streamclone-3/.hg/store/00changelog.d

Check no-upload-if-less-than-chunks option
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg" --no-upload-if-less-than-chunks 2
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 0, data: 0, repo: repo (glob)
  * has too few chunks to upload - 1. Exiting, repo: repo (glob)
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg" --no-upload-if-less-than-chunks 2 --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * current sizes in database: index: 0, data: 0, repo: repo (glob)
  * about to upload 3 entries, repo: repo (glob)
  * inserting into streaming clone database, repo: repo (glob)
  * current max chunk num is None, repo: repo (glob)
