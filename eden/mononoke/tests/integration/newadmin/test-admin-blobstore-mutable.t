# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 setup_common_config "blob_files"


Check we can upload and fetch an arbitrary blob.
  $ echo value > "$TESTTMP/value"
  $ mononoke_admin raw-blobstore --storage-name blobstore --use-mutable upload --key somekey --value-file "$TESTTMP/value"
  Writing 6 bytes to blobstore key somekey

Check it's visible in mutable but not outside of it
  $ mononoke_admin raw-blobstore --storage-name blobstore --use-mutable fetch -q somekey -o "$TESTTMP/fetched_value"
  $ diff "$TESTTMP/value" "$TESTTMP/fetched_value"

  $ mononoke_admin raw-blobstore --storage-name blobstore  fetch -q somekey -o "$TESTTMP/fetched_value"
  No blob exists for somekey


Same thing but with the repo blobstore
  $ mononoke_admin blobstore --repo-name repo --use-mutable upload --key somekey --value-file "$TESTTMP/value"
  Writing 6 bytes to blobstore key somekey

  $ mononoke_admin blobstore --repo-name repo --use-mutable fetch -q somekey -o "$TESTTMP/fetched_value"
  $ diff "$TESTTMP/value" "$TESTTMP/fetched_value"

  $ mononoke_admin blobstore --repo-name repo fetch -q somekey -o "$TESTTMP/fetched_value"
  No blob exists for somekey
