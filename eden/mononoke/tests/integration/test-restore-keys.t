# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ export FILESTORE=1
  $ export FILESTORE_CHUNK_SIZE=10
  $ REPOID=0 REPONAME=orig setup_common_config blob_files
  $ REPOID=1 REPONAME=backup setup_common_config blob_files
  $ REPOID_SRC=0
  $ REPOID_DEST=1
  $ cd $TESTTMP

  $ testtool_drawdag -R orig << EOF
  > D
  > |
  > C
  > |
  > B
  > |
  > A
  > # modify: D largefile aaaaaaaaaaaaaaaaaaaa
  > # bookmark: D master_bookmark
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  D=c7a7f3b69711451beddb85d7bcd285b13e5f42e4823d020c1cdb52f36622089f
Put list of keys reachable from master_bookmark in a file. This list was produced by enumerating the blobstore
  $ cat > "$TESTTMP"/keys <<EOF
  > repo0000.alias.gitsha1.02358d2358658574ba0767140caa4216ee7ea5bf
  > repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54
  > repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220
  > repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c
  > repo0000.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8
  > repo0000.alias.seeded_blake3.5667f2421ac250c4bb9af657b5ead3cdbd940bfbc350b2bfee47454643832b48
  > repo0000.alias.seeded_blake3.5ad3ba58a716e5fc04296ac9af7a1420f726b401fdf16d270beb5b6b30bc0cda
  > repo0000.alias.seeded_blake3.6fb4c384e79ac0771a483fcf3c46fb4ea8609f79608e8bcbf710f9887a3b9cf6
  > repo0000.alias.seeded_blake3.7d7b4da0a640276f478981b44ea9c8c8905b70fcfb3a51ad864db2278d286339
  > repo0000.alias.seeded_blake3.f855e902aa37298b838d1d19011c007df22aa7c709f2e65839a34e15268ad89b
  > repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d
  > repo0000.alias.sha1.38666b8ba500faa5c2406f4575d42a92379844c2
  > repo0000.alias.sha1.50c9e8d5fc98727b4bbc93cf5d64a68db647f04f
  > repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b
  > repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec
  > repo0000.alias.sha256.3f39d5c348e5b79d06e842c114e6cc571583bbf44e4b0ebfda1a01ec05745d43
  > repo0000.alias.sha256.42492da06234ad0ac76f5d5debdb6d1ae027cffbe746a1c13b89bb8bc0139137
  > repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd
  > repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d
  > repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c
  > repo0000.changeset.blake2.aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  > repo0000.changeset.blake2.c7a7f3b69711451beddb85d7bcd285b13e5f42e4823d020c1cdb52f36622089f
  > repo0000.changeset.blake2.e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  > repo0000.changeset.blake2.f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  > repo0000.chunk.blake2.cb4cdd1c11d9bedb6dd0f5e2d98a0b6f4544b9ed9f093cb178dd2ddfc09f6f99
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  > repo0000.content.blake2.90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0
  > repo0000.content.blake2.b90d3bbb67186b5cb22bd4af4ab7ee7566aa09d5897ae452011cf64a7c257647
  > repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  > repo0000.content_metadata2.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.content_metadata2.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d
  > repo0000.content_metadata2.blake2.90c8e211c758a9bbcd33e463c174f1693692677cb76c7aaf4ce41aa0a29334c0
  > repo0000.content_metadata2.blake2.b90d3bbb67186b5cb22bd4af4ab7ee7566aa09d5897ae452011cf64a7c257647
  > repo0000.content_metadata2.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9
  > EOF

Write one blob with corrupt content
  $ CORRUPT_BLOB_KEY_SRC_REPO="$TESTTMP"/blobstore/blobs/blob-repo0000.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8
  $ CORRUPT_BLOB_KEY_DEST_REPO="$TESTTMP"/blobstore/blobs/blob-repo0001.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8

  $ echo a > "$CORRUPT_BLOB_KEY_DEST_REPO"
  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  b19be4239976c5ddf536f26cf4b7d2e7d1196c13b5644052fcb6caf0733f2f0b  $TESTTMP/blobstore/blobs/blob-repo0000.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  87428fc522803d31065e7bce3cf03fe475096631e5e07bbd7a0fde60c4cf25c7  $TESTTMP/blobstore/blobs/blob-repo0001.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8

Check that only a single key exist before the copy command
  $ ls -al "$TESTTMP"/blobstore/blobs/blob-repo0001.* | wc -l
  1

First run should fail, because we do not strip repo0000 prefix
  $ mononoke_admin blobstore copy-keys --source-repo-id "$REPOID_SRC" --target-repo-id "$REPOID_DEST" --input-file "$TESTTMP"/keys \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 35 keys to copy (glob)
  Error: failed to copy repo0000.alias.gitsha1.02358d2358658574ba0767140caa4216ee7ea5bf
  
  Caused by:
      Not found
  [1]



Now run with ignore errors - it should not fail, but should not copy anything either
  $ mononoke_admin --blobstore-put-behaviour Overwrite blobstore copy-keys --source-repo-id "$REPOID_SRC" --target-repo-id "$REPOID_DEST" --input-file "$TESTTMP"/keys --ignore-errors \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success 2>&1 | grep -v 'failed to copy'
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 35 keys to copy (glob)
  * 3 keys processed (glob)
  * 6 keys processed (glob)
  * 9 keys processed (glob)
  * 12 keys processed (glob)
  * 15 keys processed (glob)
  * 18 keys processed (glob)
  * 21 keys processed (glob)
  * 24 keys processed (glob)
  * 27 keys processed (glob)
  * 30 keys processed (glob)
  * 33 keys processed (glob)
  * 0 keys were copied (glob)
  $ wc -l "$TESTTMP"/missing
  35 $TESTTMP/missing

  $ mononoke_admin --blobstore-put-behaviour Overwrite blobstore copy-keys --source-repo-id "$REPOID_SRC" --target-repo-id "$REPOID_DEST" --input-file "$TESTTMP"/keys --strip-source-repo-prefix \
  > --error-keys-output "$TESTTMP"/errors \
  > --missing-keys-output "$TESTTMP"/missing \
  > --success-keys-output "$TESTTMP"/success
  * using repo "orig" repoid RepositoryId(0) (glob)
  * using repo "backup" repoid RepositoryId(1) (glob)
  * 35 keys to copy (glob)
  * 3 keys processed (glob)
  * 6 keys processed (glob)
  * 9 keys processed (glob)
  * 12 keys processed (glob)
  * 15 keys processed (glob)
  * 18 keys processed (glob)
  * 21 keys processed (glob)
  * 24 keys processed (glob)
  * 27 keys processed (glob)
  * 30 keys processed (glob)
  * 33 keys processed (glob)
  * 35 keys were copied (glob)
  $ wc -l "$TESTTMP"/success
  35 $TESTTMP/success

Check that the keys were copied
  $ ls -al "$TESTTMP"/blobstore/blobs/blob-repo0001.* | wc -l
  35
Check that corrupt blob was fixed
  $ sha256sum "$CORRUPT_BLOB_KEY_SRC_REPO"
  b19be4239976c5ddf536f26cf4b7d2e7d1196c13b5644052fcb6caf0733f2f0b  $TESTTMP/blobstore/blobs/blob-repo0000.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8
  $ sha256sum "$CORRUPT_BLOB_KEY_DEST_REPO"
  b19be4239976c5ddf536f26cf4b7d2e7d1196c13b5644052fcb6caf0733f2f0b  $TESTTMP/blobstore/blobs/blob-repo0001.alias.gitsha1.a296a68b1376fedf3721260804849b1f4087f6a8
