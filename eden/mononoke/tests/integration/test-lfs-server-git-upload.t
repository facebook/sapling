# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# Create a repository
  $ setup_common_config
  $ REPOID=1 FILESTORE=1 FILESTORE_CHUNK_SIZE=10 setup_mononoke_repo_config git_test

# Create a git blob to upload
  $ git init -q git-repo
  $ cd git-repo/
  $ echo "Test blob" > git_blob.txt
  $ BLOB_OID=$(git hash-object -w -t blob git_blob.txt)
  $ BLOB_SIZE=$(stat -c %s git_blob.txt)
  $ cd ..

# Start a LFS server for this repository (no upstream)
  $ lfs_log="$TESTTMP/lfs.log"
  $ lfs_uri="$(lfs_server --tls --log "$lfs_log"  --git-blob-upload-allowed)/git_blob_upload/git_test/${BLOB_OID}/${BLOB_SIZE}"

# Confirm blobstore is empty
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_0.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_1.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"

# Uploading without a cert should fail, hard
  $ curl -s --upload-file git-repo/git_blob.txt ${lfs_uri}
  [60]

# Confirm blobstore is empty
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_0.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_1.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"

# But with a cert (and hence with authority), it should work
  $ sslcurl -s --upload-file git-repo/git_blob.txt ${lfs_uri}

# Confirm blobstore has file content chunks, and that the gitsha1 alias is correct - the alias output by echo
# should be the same as the one found in SQLite
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_0.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"
  repo0001.alias.sha256.10f7beb257a6c09c796819019a6224a4355fe88e3579c37102fd69e8435ade99|0
  repo0001.content_metadata.blake2.f2fb68f4dbe4f73cc0475785ac7e8c6d7ac0ea0cdf6244aeb1719506cd4ffd57|0
  $ echo "repo0001.alias.gitsha1.${BLOB_OID}|0"
  repo0001.alias.gitsha1.c9385e53096db9bd2395f04495c0706de072fa27|0
  $ sqlite3 "$TESTTMP/blobstore_git_test/blobs/shard_1.sqlite" "SELECT id, chunk_count FROM data ORDER BY id;"
  repo0001.alias.gitsha1.c9385e53096db9bd2395f04495c0706de072fa27|0
  repo0001.alias.sha1.48ae28c677e9a399e8ababd6e529fabcfd99028a|0
  repo0001.content.blake2.f2fb68f4dbe4f73cc0475785ac7e8c6d7ac0ea0cdf6244aeb1719506cd4ffd57|0
