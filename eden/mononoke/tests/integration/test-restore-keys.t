# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ FILESTORE=1
  $ FILESTORE_CHUNK_SIZE=10
  $ REPOID=0 REPONAME=orig setup_common_config blob_files
  $ REPOID=1 REPONAME=backup setup_common_config blob_files
  $ REPOID_SRC=0
  $ REPOID_DEST=1
  $ cd $TESTTMP

  $ hg init repo-hg
  $ cd repo-hg
  $ setup_hg_server
  $ drawdag <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ hg up -q tip
  $ echo "aaaaaaaaaaaaaaaaaaaa" > largefile
  $ hg add largefile
  $ hg ci -m 'large commit'
  $ hg book -r . master_bookmark
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

Put list of keys reachable from master_bookmark in a file. This list was produced by running
walker
  $ cat > "$TESTTMP"/keys <<EOF
  > repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54
  > repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220
  > repo0000.alias.gitsha1.8ded189dea5fbb72e4cbcad97206f50bd44a53c9
  > repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c
  > repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d
  > repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b
  > repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec
  > repo0000.alias.sha1.f53126f18c2571ecec28ccf968fa9f9a7d6904d8
  > repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd
  > repo0000.alias.sha256.6989db0c1f6aff311bf2e3a84a0f986bb8c9d091d95f69b40309f1f93a5e7b5c
  > repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d
  > repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c
  > repo0000.changeset.blake2.226a03f7ab80550686fbdce35e2afdabaf4bbbd266c6062a59f750b08dfd7c26
  > repo0000.chunk.blake2.7ae58bd0de68574c7637ea0a7574b41076815bf37846a2aa0981092b979ee769
  > repo0000.chunk.blake2.cb4cdd1c11d9bedb6dd0f5e2d98a0b6f4544b9ed9f093cb178dd2ddfc09f6f99
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  > repo0000.content.blake2.d9b44f78244b55e187b398ffb891da54e595dd86ff9cc6dfed8a8378e6c83c5d
  > repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  > repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  > repo0000.content_metadata.blake2.d9b44f78244b55e187b398ffb891da54e595dd86ff9cc6dfed8a8378e6c83c5d
  > repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  > repo0000.hgchangeset.sha1.a5c395b7e77e6dd5afdc0a3d042c9ed9fe913c27
  > repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9
  > repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  > repo0000.hgfilenode.sha1.51c7950483cd61d55482692dc403779594ea6f09
  > repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3
  > repo0000.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5
  > EOF

Write one blob with corrupt content
  $ CORRUPT_BLOB_KEY_SRC_REPO="$TESTTMP"/blobstore/blobs/blob-repo0000.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5
  $ CORRUPT_BLOB_KEY_DEST_REPO="$TESTTMP"/blobstore/blobs/blob-repo0001.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5

  $ echo a > "$CORRUPT_BLOB_KEY_DEST_REPO"
  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  612456230aabc07a5b9530a0b6a31ac6b4ea9548268e889e698158c6a01b8c3b  $TESTTMP/blobstore/blobs/blob-repo0000.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7  $TESTTMP/blobstore/blobs/blob-repo0001.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5

Check that only a single key exist before the copy command
  $ ls -al "$TESTTMP"/blobstore/blobs/blob-repo0001.* | wc -l
  1

First run should fail, because we do not strip repo0000 prefix
  $ copy_blobstore_keys "$REPOID_SRC" "$REPOID_DEST" --input-file "$TESTTMP"/keys \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 29 keys to copy (glob)
  Error: failed to copy repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54
  
  Caused by:
      Not found
  [1]


Now run with ignore errors - it should not fail, but should not copy anything either
  $ copy_blobstore_keys "$REPOID_SRC" "$REPOID_DEST" --input-file "$TESTTMP"/keys --ignore-errors \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success 2>&1 | grep -v 'failed to copy'
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 29 keys to copy (glob)
  * 2 keys processed (glob)
  * 4 keys processed (glob)
  * 6 keys processed (glob)
  * 8 keys processed (glob)
  * 10 keys processed (glob)
  * 12 keys processed (glob)
  * 14 keys processed (glob)
  * 16 keys processed (glob)
  * 18 keys processed (glob)
  * 20 keys processed (glob)
  * 22 keys processed (glob)
  * 24 keys processed (glob)
  * 26 keys processed (glob)
  * 28 keys processed (glob)
  * 0 keys were copied (glob)
  $ wc -l "$TESTTMP"/missing
  29 $TESTTMP/missing

  $ copy_blobstore_keys "$REPOID_SRC" "$REPOID_DEST" --input-file "$TESTTMP"/keys --strip-source-repo-prefix \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 29 keys to copy (glob)
  * 2 keys processed (glob)
  * 4 keys processed (glob)
  * 6 keys processed (glob)
  * 8 keys processed (glob)
  * 10 keys processed (glob)
  * 12 keys processed (glob)
  * 14 keys processed (glob)
  * 16 keys processed (glob)
  * 18 keys processed (glob)
  * 20 keys processed (glob)
  * 22 keys processed (glob)
  * 24 keys processed (glob)
  * 26 keys processed (glob)
  * 28 keys processed (glob)
  * 29 keys were copied (glob)
  $ wc -l "$TESTTMP"/success
  29 $TESTTMP/success

Check that the keys were copied
  $ ls -al "$TESTTMP"/blobstore/blobs/blob-repo0001.* | wc -l
  29
Check that corrupt blob was fixed
  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  612456230aabc07a5b9530a0b6a31ac6b4ea9548268e889e698158c6a01b8c3b  $TESTTMP/blobstore/blobs/blob-repo0000.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  612456230aabc07a5b9530a0b6a31ac6b4ea9548268e889e698158c6a01b8c3b  $TESTTMP/blobstore/blobs/blob-repo0001.hgmanifest.sha1.71921e5083c11d8a6e17fa9a0295612ead5069d5
