# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

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

  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * about to upload 1 entries (glob)
  * inserting into streaming clone database (glob)

Try creating again, this should fail
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * cannot create new streaming clone chunks because they already exists (glob)
  [1]

  $ mononoke
  $ wait_for_mononoke

  $ hgmn clone --stream ssh://user@dummy/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 357 bytes of data
  transferred 357 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at:* (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ diff repo-streamclone/.hg/store/00changelog.i repo-hg/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo-hg/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-d1de0dadf747295f0e1ea4db829b8e87437476f94cefcb948cd3b366b599d49e5a7c74b2777372b74c4962c513f71c72252bf673a8c880387ea84a5317abb14b-idx|streaming_clone-chunk000000-a5750ff674daa16106403d02aebff7d19ad96a33886c026427002f30c9eea7bac76387c4dd5f5c42a9e3ab1ecd9c9b5d3c2a079406e127146bddd9dcc8c63e23-data

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "delete from streaming_changelog_chunks where repo_id = 0;"
  $ streaming_clone create --dot-hg-path "$TESTTMP/repo-hg/.hg" --max-data-chunk-size 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * about to upload 3 entries (glob)
  * inserting into streaming clone database (glob)
  $ rm -rf "$TESTTMP/repo-streamclone"
  $ cd "$TESTTMP"
  $ hgmn clone --stream ssh://user@dummy/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  streaming all changes
  2 files to transfer, 357 bytes of data
  transferred 357 bytes in * seconds (*) (glob)
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ diff repo-streamclone/.hg/store/00changelog.i repo-hg/.hg/store/00changelog.i
  $ diff repo-streamclone/.hg/store/00changelog.d repo-hg/.hg/store/00changelog.d
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select idx_blob_name, data_blob_name from streaming_changelog_chunks where repo_id = 0 order by chunk_num asc;"
  streaming_clone-chunk000000-760d25b269f0be9a5ef3aabd126ef025d5f13d279da46d9721d8a07423dc9ba03be1acceb23e6f0ccd9bdc330bc2911ff386f1444cdf279ee0506368013792be-idx|streaming_clone-chunk000000-31d5f335f6e9ac058258e7d242402d6d0f218f075647b8aa9caee655127f66b1954236f46b1f0c19cf837ff9a80651f4f5681ace3bea083437f310d2ef92cf3e-data
  streaming_clone-chunk000001-ddc1b4ac17d56e27b899602bca51925d3fdfd21a1defc05ecf83c1d7b3ef2e0c4c9a3cb3a6e412936019888259d39b62f89bafe0af101f29d0eb189b9b528cfd-idx|streaming_clone-chunk000001-ff9763a4f2f9bce3bef31a9e03814d59e6d78b77371024d9b613f8a5829efe21d75fbf7374f1b7219c87ece7805246b7c0a74128b9d48f84e8840bf6ebf65249-data
  streaming_clone-chunk000002-b1d37871e4f34fab6d57b7bf076da07028a69d1a70e4bccca0721013e5eeaa3a2fcdf61785b808b8ecee68a69782be16bdee4f51d94eeb3bf82b10d7796a55b2-idx|streaming_clone-chunk000002-a4daa490eef701a7b0e2bd86eb1579500011a67f7d1eaf148bff6b1095dcd3278e8e0bf940b1a5b8db4af990d5edbed0f752c7513f50627b206749104c71ab4b-data
